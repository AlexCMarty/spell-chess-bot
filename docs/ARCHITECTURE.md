# Architecture

Orientation for the Rust in `crates/`. For the *rules* this engine implements, start at
[`rules/INDEX.md`](../rules/INDEX.md) instead — this file assumes you already know that a
turn is one optional spell plus one mandatory move. For the browser build — `crates/wasm`,
the worker protocol, the JSON contract with `web/` — see [`WEB.md`](WEB.md).

## The idea that shapes everything

In orthodox chess a move generator answers "which moves are legal?". Here the unit of play
is a **`Turn`**: an optional `SpellCast` plus a mandatory `PieceMove`. A spell is cast
*before* the move and can change the legality of that same move — freezing your own piece,
dispelling a check against you, or opening a line that did not exist a moment ago.

So the naive generator is: for every legal spell target, build the hypothetical position
that results, run a full legality scan against it, and pair each surviving move with that
cast. That is correct and it is what `generate_turns` still does. It is also
catastrophically slow — a full rescan per spell target, at every node.

**Most of the complexity in this codebase is the work of not doing that rescan**, while
staying provably identical to it. Once you see that, the module layout follows.

## Crates

| Crate | Depends on | Holds |
|---|---|---|
| `crates/core` | nothing internal | Board, move generation, spells, legality. The rules engine. |
| `crates/search` | `core` | Alpha-beta with quiescence, transposition table, move ordering, evaluation. |
| `crates/cli` | `core` + `search` | The `spellchess` REPL binary. |
| `crates/wasm` | `core` + `search` | The browser boundary. `lib.rs` is the `wasm-bindgen` shell (its bulk is `#[cfg(target_arch = "wasm32")]`, so **no native build compiles it**); `view.rs` is the testable JSON layer. See [`WEB.md`](WEB.md). |
| `crates/core/fuzz` | `core` | `cargo-fuzz` targets. **Own `[workspace]` table — not built or tested by `--workspace`.** |

## Where the difficulty actually lives

Two files are more than half the hard code:

- **`core/src/spell_delta.rs`** (~1,470 lines) — computes the captures a spell newly makes
  legal *directly from bitboards*, instead of building a hypothetical position and
  rescanning it. This is the optimization the whole engine is built around, and it is
  where every legality bug in this project's history has lived.
- **`core/src/legal.rs`** (~1,410 lines) — full legality: `legal_moves`, `legal_captures`,
  and the six turn generators below. It builds a `NodeContext` — defined in
  `spell_delta.rs`, constructed here, once per node — which precomputes the per-node
  state (frozen set, jump fields, slider occupancy, checkers, pins) that the delta path
  reads.

Everything else is comparatively mechanical: `bitboard.rs`, `board.rs`, `types.rs`,
`rays.rs`, `attacks.rs`, `movegen.rs` (pseudo-legal generation), `fields.rs` (live spell
fields), `spells.rs` (spell targeting + relevance filtering), `terminal.rs` (win/draw).

## The turn-generator family

Six generators, and picking the wrong one is a correctness *or* a performance bug. They
differ along two axes: how complete the spell coverage is, and how expensive.

| Generator | Used by | Spell coverage |
|---|---|---|
| `generate_turns` | CLI, `terminal.rs` | **Exhaustive.** All spell targets, full rescan. The reference oracle. |
| `generate_search_turns` | `best_turn`, the depth-0 terminal check, and the out-of-time root fallback — **not** the main node expansion | Relevance-filtered targets, paired with the full baseline |
| `generate_search_spell_turns` | search | Spell-paired turns only, no no-spell copies — so a beta cutoff can skip the expensive pairing |
| `generate_quiescence_turns` / `_turns_from` | quiescence **entry** | Captures, including jump- and freeze-enabled *new* captures. Expensive: a scan per relevant target. |
| `generate_quiescence_recapture_turns` | quiescence **recursion** | Captures plus "freeze the recapturer" only. Cheap, pure bitboard. |

A node in `alphabeta` does not call any single generator from this table: it builds its
baseline with `legal_moves` + `no_spell_turns`, then adds either
`generate_search_spell_turns` or a capture-filtered `spell_capture_turns`, chosen by the
`full_spells` switch in `search.rs`.

The entry/recursion split exists because using the expensive generator at every recursive
quiescence node once made a depth-6 search 40x slower. The expensive cases are covered
once at entry.

`spells.rs` supplies the target sets: exhaustive `freeze_targets`/`jump_targets`, and
relevance-filtered `relevant_freeze_targets`/`relevant_jump_targets`. **Relevance
filtering is not universally cheaper** — see the dead ends in the `perf-measurement`
skill before swapping one for the other.

## The fast-path / oracle pattern

`spell_delta.rs` is a fast path that must agree exactly with the slow rescan. Two rules
make that safe, and both are load-bearing:

1. **The slow path is never deleted.** It remains as the differential oracle every test
   compares against.
2. **The fast path may decline.** `Delta::NeedsRescan` lets it say "I can't model this
   geometry" and fall back, rather than guessing. Shipping an incomplete fast path is
   only safe because declining is always available.

Follow this pattern for any new fast path. Do not extend the delta to a case you cannot
prove; return `NeedsRescan`.

## Invariants

- **Node counts are the perf gate.** A performance change must leave node/qnode counts
  bit-identical. If they move, you changed the search tree, which is a correctness change
  needing review — not a benchmark. Speed work must not narrow the tree; that is settled
  policy.
- **Emission order is part of the generator contract**, not just the set. Quiescence
  iterates its list unsorted, and `order_turns` sorts with `sort_unstable_by_key`, so
  emission order also breaks ties among equal-priority turns. Either way a different
  order changes beta-cutoff order and qnode counts.
  `pseudo_legal_moves` emits **castling first**, then each own non-frozen square in
  ascending square index, and within a square in fixed per-`PieceKind` order (pawn:
  single push, double push, then captures). It is **not** sorted by `(from, to)` —
  castling is emitted from `e1` before the `a1` rook's moves, and a pawn emits its
  double push before its captures. `pseudo_legal_captures` mirrors that relative order
  exactly; the property test
  `pseudo_legal_captures_matches_pseudo_legal_moves_filtered_ordered` in `movegen.rs`
  pins it. A replacement must reproduce this order, not a sorted one.
- **Never take a transposition-table cutoff at ply 0**, and record the root's best move as
  it is found rather than reading it back out of the TT afterwards. Both were real bugs.
- **Quiescence never probes the TT.** ~87% of nodes are quiescence nodes, which is why
  Lazy-SMP does not pay here (see the `perf-measurement` skill).
- **A node that searched a truncated spell list may not store an `Exact` or `Upper`
  bound.** `search.rs`'s `alphabeta` decides `full_spells` from
  `ply == 0 || in_check || (is_pv && depth <= 3) || no_spell_empty`; when it is false the
  node skips the TT store **entirely** unless it failed high (`if !(full_spells || best >=
  beta) { return Some(best); }`). A truncated list can only miss a *better* move, so the
  score is a valid `Lower` bound and nothing else. Deleting that early return as a
  "why does this node sometimes not store?" cleanup poisons the table with scores that
  can only be underestimates.
- **Null-move pruning is disabled whenever any of our own pieces is frozen.**
  `search.rs` computes `frozen_us` from `spells::frozen_bb(pos)` intersected with the
  side to move and gates the null move on `!frozen_us`. This reads like an optimisation
  gate and is a correctness guard: a frozen side can sit in zugzwang-like states that the
  null move models wrongly. Do not widen it.
- **Spell and no-spell pairings of the same move are not duplicates.**
  `dedup_captures` in `search.rs` must keep two turns with identical `from`/`to`/
  `promotion` that differ only in their spell. Collapsing them on the move triple alone —
  which is exactly what a dedup helper looks like it ought to do — drops the
  freeze-then-capture and jump-then-capture pairings that quiescence exists to see.

## Tests

| Path | Role |
|---|---|
| `core/tests/*_soundness.rs` | Differential batteries: fast path vs. the slow oracle, over generated positions |
| `core/tests/oracle_vectors.rs` | Positions verified against chess.com's own engine |
| `core/tests/legal_captures.rs` | Hand-derived expected outputs |
| `search/tests/search_identity.rs` | Best turn + score stability |
| `crates/core/fuzz/` | `cargo-fuzz` targets, run manually |

Read the `spell-legality-testing` skill before writing tests here. The short version: the
generated batteries are a regression net, not a discovery tool — every legality bug this
project has shipped was found by a hand-built adversarial geometry or by mutation-testing
a guard, never by a uniform-random battery.

## Performance harnesses

`bench` (the number that counts), `hotcost` (per-call cost), `qprof` (where that cost
goes, behind `--features spellchess-core/qprofile`), and `SPELLCHESS_PROFILE=1` for node
counters. They answer different questions and have disagreed with each other. See the
`perf-measurement` skill for which to trust when.

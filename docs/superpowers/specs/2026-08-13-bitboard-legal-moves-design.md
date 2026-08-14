# Bitboard rewrite and checkers/pins `legal_moves` — design

Date: 2026-08-13
Status: approved

## Goal

Make dense-opening search fast enough that **depth 3 on the starting position finishes
in a few seconds** (release build, same `go --depth N` protocol as the 2026-08-12 and
2026-08-13 passes). Current numbers on this machine, after relevance-filtered
`generate_search_turns` and the move-ordering/`FieldSet` work:

| search | current |
|---|---|
| depth 1 | ~2.7s |
| depth 2 | ~31s |
| depth 3 | does not finish in 90s |

The remaining cost is per-node generation, not search algorithm: `legal_moves` clones
the position and reruns `is_square_attacked` for every pseudo-legal move, and
`generate_turns_from` repeats that whole pass once per relevant freeze/jump target.
`walk_ray` allocates a `Vec` per ray; `king_square` scans all 64 squares; `Board` is
`Clone` but not `Copy`.

This design replaces the mailbox attack/movegen hot path with bitboards and rewrites
`legal_moves` to filter against precomputed checkers and pins, instead of
clone-and-rescan.

## Non-goals

- **No magic bitboards.** Slider attacks are bitscan rays over `occupancy & !jump`.
  Magics work with that occupancy too, but they add tables and extra surface area we
  do not need on a Pi 5 for this target. Follow-up only.
- **No this-ply freeze/jump reuse filter in this pass.** `generate_turns_from` still
  calls `legal_moves` once per relevant spell target. If measurement misses the depth-3
  bar, that filter is the explicit fallback (see Measurement), not silent scope creep.
- **No rewrite of eval to iterate bits.** `crates/search` eval and ordering keep
  calling `board.get`.
- **No search-algorithm changes.** TT, killers, history, alpha-beta, and
  `generate_search_turns` vs `generate_turns` semantics stay as they are.
- **No change to `generate_turns`'s exhaustive behavior.** It remains the CLI / mate
  oracle. It just gets faster `legal_moves` underneath.
- **No bitboard-only `Board`.** Mailbox stays so existing `get`/`set` call sites
  (tests, CLI, eval) do not all have to move at once.

## Board representation

`Square(rank * 8 + file)` already matches the usual A1 = bit 0 mapping. Keep it.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Bitboard(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Board {
    squares: [Option<Piece>; 64], // mailbox; get/set stay O(1)
    by_color: [Bitboard; 2],
    by_kind: [Bitboard; 6],
    kings: [Option<Square>; 2],
}
```

Occupancy is `by_color[White] | by_color[Black]`. `Board::set` is the **only** writer:
it updates mailbox, both bitboard arrays, and the king cache together. In debug/test
builds, `set` (or a `debug_assert` helper it calls) checks that mailbox, bits, and king
cache agree. A desync panics; it must not become a quiet wrong move.

`Position` becomes `Copy` (`FieldSet` already is). `apply_move_only` is then a memcpy
plus a few `set`s — no heap, no 64-square king scan.

`king_square` returns the cache. `None` remains a first-class terminal (that king was
captured); it is not an error.

## Spell masks

Precompute `FREEZE_ZONE: [Bitboard; 64]` — the 3×3 (edge-clipped) mask for a freeze
anchor, matching `spells::freeze_zone`.

Derived from live fields, once per generation call:

- `frozen(pos)` = OR of `FREEZE_ZONE[anchor]` over active freeze fields. Frozen pieces
  **still occupy**. They do not move and do not attack (`rules/30-freeze.md`).
- `jump(pos)` = bits of live jump anchors. Slider occupancy is `occupancy & !jump`.
  The jumped square stays occupied in the mailbox, so a slider can **capture the piece
  on it or pass through** (`rules/40-jump.md`).
- Pawn double-step: the mid-square is passable if empty **or** set in `jump`.

`is_square_frozen` / `is_square_jump_active` become bit tests on these masks.
`FieldSet::push` still panics past capacity 4.

## Attacks and pseudo-legal movegen

Replace `walk_ray`'s allocated `Vec` with bitscan in a direction, stopping at the first
set bit of slider occupancy (`occupancy & !jump`). Knight, king, and pawn attack tables
are precomputed `[Bitboard; 64]` (pawns per color).

`is_square_attacked` / `attacker_count` become queries against attacks **generated from
pieces**, not a walk outward from the target. A slider standing *on* a jump square still
attacks from there: its origin is not treated as a blocker. That is the bitboard fix
for the walk-from-target bug recorded in `relevance_soundness` / jump-relevance work
(a slider on a jump square was invisible to `walk_ray(...).last()`).

Frozen enemy pieces contribute nothing to the attack map. They still appear in
occupancy, so they still block sliders unless their square is also jumped.

`pseudo_legal_moves` iterates `own & !frozen`. Destinations are empty or enemy
(mailbox decides which). Castling keeps today's rules: empty path, rook present and
not frozen, king and through-squares not attacked. A piece may move *into* a freeze
zone; it may not *leave* a frozen origin.

`rays::walk_ray` becomes a compatibility helper on top of bitboard attacks, or is
removed once its call sites (`attacks.rs`, `spells.rs` relevance, `eval.rs` jump-threat)
have moved. Relevance-filter logic in `spells.rs` must keep the same target sets; only
the ray primitive underneath changes.

## Checkers, pins, and `legal_moves`

Production `legal_moves` no longer clones every candidate. Once per position:

1. **Checkers** — unfrozen enemy pieces that attack our king, sliders on
   `occupancy & !jump`.
2. **Pins** — along each king ray, if occupancy-for-sliders sees one friendly piece
   and then an unfrozen enemy slider, that friendly is pinned to the ray mask. A piece
   *on* a jump square is not a blocker, so that geometry is a jump-through check, not
   a pin.
3. **Evasion mask** (does not apply to king-capture moves; those are a separate
   branch below, and must be considered *before* “double check → king moves only” —
   `dense_01` allows an unrelated piece to take the enemy king while already in
   double check):
   - No check: all destinations.
   - Double (or more) check: king moves only.
   - Single contact check (knight, pawn, adjacent king, or slider on an adjacent
     square): capture the checker or king move; no interposition.
   - Single slider check that does **not** pass through a live jump: capture the
     checker or interpose on the between-mask.
   - Jump-through check: capture the checker or king move only — no interposition on
     or behind the jump square (`rules/40-jump.md`).

Filter:

- **King capture** (`dest` is the enemy king): first. Keep the measured rule
  `attackers_after <= max(attackers_before, 1)` (`rules/50-interactions.md`,
  `rules/10-base-chess.md`). This path is rare, so it may `apply_move_only` (now a
  copy) and recount. Do not encode it in the pin algebra. Do not reject it for
  double check.
- **King moves:** destination not attacked. When testing that, XOR the king off
  occupancy so enemy sliders x-ray the vacated square; frozen enemies still do not
  attack.
- **Non-king:** destination must be in the evasion mask; a pinned piece must stay on
  its pin ray.
- **En passant:** rare, and discovered check on the rank is easy to get wrong. Copy
  and recheck that one move. A frozen EP victim stays capturable
  (`rules/30-freeze.md`).

If a spell interaction is too subtle to express in pins, that move stays on the
copy-and-recheck path. A rare slow path is better than a silent illegal.

`generate_turns` / `generate_search_turns` keep today's shape: one baseline
`legal_moves`, then `position_with_field` + `legal_moves` per freeze/jump target.
That is cheaper because `Position` is `Copy` and `legal_moves` no longer
clone-and-rescans. Target lists and exhaustive-vs-filtered semantics do not change.

## Test oracle

Current clone-and-rescan stays as `legal_moves_reference`, **tests only**: generate
pseudo-legal moves, `apply_move_only` each, ask `is_square_attacked` / `attacker_count`.
It is not a frozen copy of today's `walk_ray`. After rollout step 2 it shares the new
bitboard attacks with production; the step-4 fuzz then isolates the checkers/pins
*filter* against that shared attack primitive. Production `legal_moves` must match the
reference exactly — not a subset, not a superset. A mismatch is a failed test, not a
performance tradeoff.

Battery:

- Existing unit tests, `oracle_vectors`, and `relevance_soundness` keep their
  contracts.
- New differential fuzz, same shape as `relevance_soundness.rs`: random legal walks,
  tens of thousands of positions, `legal_moves` vs reference, **including
  hypotheticals with an extra freeze or jump field**. Pins × freeze × jump is where
  the last three rule bugs lived (same-square recast, king-capture-vs-double-check,
  zero-legal-move target exclusion).
- King-capture fixtures (`dense_01`, `dense_03`, and the three measured cases in
  `rules/50-interactions.md`) are explicit regressions, not only fuzz.

Do not keep two movegens in production. The reference exists solely to pin the rewrite.

## Invariants

- `Board::set` is the only mutator of occupancy / mailbox / king cache.
- Missing king is a win for the other side, not an error.
- `FieldSet` capacity-4 panic unchanged.
- Production legality must equal the reference on every fuzz position, including
  spell hypotheticals.

## Measurement

Same protocol as the last two passes: release build, starting position, `go --depth N`
on this machine.

| search | current | target |
|---|---|---|
| depth 1 | ~2.7s | tens of milliseconds |
| depth 2 | ~31s | well under 1s if depth 3 is in range |
| depth 3 | timeout at 90s | **a few seconds** (treat as **under 5s** for the ignored test bound) |

Update `depth_budget_stays_bounded_on_a_realistic_board` so depth 1 stays a regression
guard, and add an ignored depth-3 bound (same `cargo test -p spellchess-search --release
-- --ignored` pattern).

If depth 3 is still tens of seconds after this rewrite, do not ship a “maybe faster”
story. Add the this-ply freeze/jump reuse filter as a follow-up: still *emit* every
relevant spell turn (the resulting position differs), but skip recomputing `legal_moves`
when the spell cannot change this ply’s move set.

## Rollout

1. `Bitboard` newtype, freeze-zone tables, hybrid `Board` + `Copy` `Position`.
   `get`/`set`/`king_square` keep working. Existing tests stay green with no movegen
   change.
2. Replace `walk_ray` / `is_square_attacked` / `attacker_count` with bitboard attacks.
   `attacks.rs` / `rays.rs` unit tests plus oracle vectors.
3. Bitboard `pseudo_legal_moves`. Movegen tests plus oracle destinations.
4. Checkers/pins `legal_moves`, reference clone-and-rescan behind tests, differential
   fuzz (including extra freeze/jump fields). **Correctness gate** — nothing after it
   proceeds on a red fuzz.
5. Wire search as-is (`generate_search_turns` unchanged). Remeasure depth 1/2/3.
   Tighten the ignored perf test.

## Scope

Confined to `crates/core` (board, rays/attacks, movegen, legal, spell mask helpers) plus
test-only reference/fuzz and the search crate’s ignored timing test. `crates/cli` is
untouched. Eval and ordering keep `board.get`.

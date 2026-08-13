# Search move-ordering and `Position.fields` representation — design

Date: 2026-08-13
Status: approved

## Goal

Improve general search quality/throughput in `crates/search`, and cut a specific fixed
cost that's paid on every one of the very large number of `Position` clones the search
tree produces. This is a narrower, lower-risk follow-up to the 2026-08-12
branching-factor work — see project memory `spell-chess-engine-followups` — which left
the dense-opening case (depth-2 ~34s, depth-3 doesn't finish in 90s on the starting
position) unaddressed because its actual fix (rewriting `legal_moves` to use
precomputed checkers/pins instead of clone-and-rescan-per-move) was judged too risky to
bundle in without its own dedicated fuzz-testing pass. That rewrite remains a deferred
follow-up (see Non-goals).

This design covers two independent changes that can land in either order or separately,
bundled here because they were scoped together in one brainstorming session:

1. `crates/search`: TT-move-first ordering, killer moves, and a history heuristic —
   standard alpha-beta move-ordering improvements that reduce the number of nodes
   visited, independent of per-node cost.
2. `crates/core`: replace `Position.fields: Vec<SpellField>` with a fixed-capacity,
   `Copy` `FieldSet`, removing the heap allocation that `Position::clone()` currently
   pays every time (which happens an extreme number of times across the search tree —
   `legal_moves`'s legality filter clones once per pseudo-legal move considered, and
   `generate_turns`/`generate_search_turns` reruns that whole pass once per relevant
   spell target).

## Non-goals

- **No rewrite of `legal_moves`'s clone-and-rescan-per-move architecture.** That's the
  checkers/pins precomputation mentioned above — the highest-ceiling fix for the dense
  opening specifically, but it touches the exact area (`legal.rs`/`attacks.rs`
  check/pin detection interacting with freeze/jump transparency) where 3 subtle rule
  bugs were already found by oracle fuzzing (same-square recast, king-capture-vs-double-
  check, zero-legal-move target exclusion — see project memory
  `spell-chess-engine-followups`). It needs its own focused differential-fuzz-testing
  pass, tracked as a follow-up in project memory, not bundled here.
- **No aspiration windows.** Smaller win than TT-move/killer/history ordering, with its
  own fail-low/fail-high re-search edge cases. Explicitly deferred by choice in this
  session; a candidate for a future quick follow-up once sections 1-4 below are measured.
- **No change to `generate_turns`'s or `generate_search_turns`'s exhaustive-vs-filtered
  behavior.** Both keep their current semantics; this design only changes what
  `Position.fields` is made of, not what it contains or when.
- **No TT replacement/aging policy.** `TranspositionTable` stays a plain `HashMap`,
  recreated per top-level `search()`/`negamax()` call, as it is today. Out of scope.

## `FieldSet`: fixed-capacity replacement for `Position.fields`

**Concurrency bound.** Grepping every existing test and every oracle fixture
(`crates/core/tests/fixtures/*.json`) confirms the steady-state invariant: **at most 1
concurrent field** is ever present in a real, reached `Position`. This follows from the
field lifetime rule (`expires_after_ply = cast_ply + 1`, purged by
`apply_turn`'s `fields.retain` on the very next ply transition) combined with "one
spell per turn, per player": a field cast on the mover's turn survives only long enough
to be visible to the opponent's immediate reply, and is purged in that same reply's
`apply_turn` call — before a third field could ever coexist with it.

The one place fields can transiently exceed 1 is `legal::position_with_field`, which
clones `pos` (already satisfying the ≤1 invariant, since real positions only ever reach
`generate_turns_from` via `apply_turn`) and pushes one *additional* hypothetical field
without calling `retain` — so a momentary maximum of 2. This hypothetical is never fed
back into `position_with_field` again (no nesting), so 2 is also the ceiling for that
path.

**Chosen capacity: 4.** This gives a 2x safety margin over the demonstrated momentary
maximum of 2, rather than encoding the tightest possible bound — cheap insurance against
a future rule change (e.g. a spell interaction that legitimately allows a third
transient field) silently becoming a capacity bug instead of a loud one.

**Shape.** `SpellField` is already `#[derive(Clone, Copy, ...)]`, so:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FieldSet {
    fields: [Option<SpellField>; 4],
}
```

`FieldSet` is therefore `Copy`, and `Position::clone()`'s `fields` member becomes a
stack copy — no allocation — regardless of how many times `Position` is cloned per node.

**API surface**, matching the `Vec` methods actually used at call sites today (`push`,
`retain`, `iter`, `is_empty`; `len` for completeness) so the ~15 existing call sites in
`legal.rs`, `attacks.rs`, `terminal.rs`, and test modules need no logic changes — only
`pos.fields.push(x)` etc. continue to compile unchanged against the new type:

- `push(&mut self, field: SpellField)` — inserts into the first `None` slot; **panics**
  if the set is already full. A silent drop would corrupt game state invisibly (a field
  that should be immobilizing something quietly isn't); a panic surfaces the invariant
  violation immediately, in testing, long before it could reach a real game.
- `retain(&mut self, f: impl FnMut(&SpellField) -> bool)` — same semantics as
  `Vec::retain`, implemented over the fixed slots.
- `iter(&self) -> impl Iterator<Item = &SpellField>` — filters out `None` slots.
- `is_empty(&self) -> bool`, `len(&self) -> usize`.
- `Default`/`new()` — all-`None`, matching `Vec::new()`'s empty start.

`Position.fields`'s field type changes from `Vec<SpellField>` to `FieldSet`; the field
stays `pub` with the same name.

## Search move ordering

### TT-move-first

Add a field to the existing TT entry:

```rust
pub struct TtEntry {
    pub depth: u32,
    pub score: i32,
    pub bound: Bound,
    pub best_move: Option<Turn>,
}
```

`Turn` is already `Copy`, so this costs nothing extra on insert. `alphabeta` currently
probes the TT only to potentially return early (`entry.depth >= depth` with a bound that
cuts). When the probe doesn't short-circuit — either `entry.depth < depth`, or the bound
doesn't cut — the entry's `best_move`, if present, is still useful as a move-ordering
hint: pull it out and pass it to `order_turns`, which places it first, ahead of the
capture/quiet heuristic, if it's present in the turn list for this node. `alphabeta`'s
existing `tt.insert(...)` call gains the actual best move found at that node (the `turn`
corresponding to `best`), threaded through the existing alpha-improvement tracking.

This is the highest-value single change here: it's what makes each iterative-deepening
pass cheap, by re-trying the previous (shallower) iteration's best line first at every
node, not just the root.

### Killer moves

Two slots per remaining-depth bucket, indexed by the `depth: u32` parameter already
threaded through `alphabeta` (not ply-from-root — collisions across different branches
that happen to share a remaining-depth value are the standard, accepted approximation
used by essentially every engine that implements this; it doesn't need to be exact to be
useful). Table shape: `Vec<[Option<Turn>; 2]>`, sized to the search's max depth and
owned by the top-level `search()`/`negamax()` call, threaded down through `alphabeta` as
`&mut`.

When a *quiet* move (no capture — same `is_capture` check `dedup_captures` already uses)
causes a beta cutoff (`alpha >= beta` in the existing loop), store it in that depth's
slot pair, evicting the older of the two if both are occupied and the new move isn't
already present. `order_turns` ranks a turn matching either killer for the current depth
above other quiet moves, below captures and the TT move.

### History heuristic

A `[[u32; 64]; 64]` table, indexed by `(from, to)`, owned alongside the killer table on
the `search()`/`negamax()` stack and threaded down the same way. On the same beta-cutoff
trigger as killers (quiet move, cutoff), increment `table[from][to] += depth * depth`.
`order_turns`'s current flat "+50 for any quiet move" term is replaced by a score derived
from this table for quiet, non-killer moves — giving real move-quality signal instead of
a constant. Table is fresh per top-level search call (reset each time, not persisted
across unrelated searches) — the simplest correct choice, avoiding stale bias carried
over from a previous, unrelated position.

### `order_turns` signature change

`order_turns` currently takes `(pos, turns)`. It needs the TT move, killer pair, and
history table to rank correctly, so its signature grows to accept them (as `Option`s /
references so call sites without a live search context — e.g. any future non-search
caller — can pass "none of the above" and fall back to today's capture-first-only
behavior). Exact parameter shape is an implementation-plan detail, not fixed here.

## Testing

- **`FieldSet`**: a focused unit-test module mirroring the `Vec`-based tests it
  replaces — push/retain/iter/is_empty/len — plus a full, unmodified re-run of the
  existing `crates/core` test suite (all rule-vector tests, `oracle_vectors.rs`,
  `relevance_soundness.rs`'s differential fuzzing) as a correctness gate. This change
  must not alter any observable rule behavior; the fuzz suite is the strongest available
  check of that, per the "trust the differential suite" lesson recorded in project
  memory.
- **Move ordering / killers / history**: unit tests in the style of the existing
  `crates/search/src/ordering.rs` tests — e.g. TT move (when present) sorts first;
  killer match sorts above a non-killer quiet move and below any capture; the existing
  `captures sort before quiet moves` and `ordering preserves the turn set` invariants
  keep holding.
- **Perf**: re-measure `go --depth 1` / `go --depth 2` on the starting position the same
  way the 2026-08-12 work did (this session's baseline: ~2.7s / ~34s release-build,
  recorded 2026-08-13), and record the before/after numbers in the commit message or
  follow-up memory. Not a guaranteed dramatic win the way the branching-factor fix was —
  this design targets *nodes visited* and *per-clone cost*, not the per-node generation
  cost that dominates the dense-opening case — but should measurably help both the
  dense-opening case (fewer nodes, cheaper clones) and general mid-game search quality
  (better move ordering matters more once positions are less symmetric).

## Scope boundaries

Confined to `crates/core` (`FieldSet` only — no movegen/legality logic changes) and
`crates/search` (`TtEntry`, `ordering.rs`, killer/history tables, `alphabeta`/`search`
wiring). `crates/cli` is untouched.

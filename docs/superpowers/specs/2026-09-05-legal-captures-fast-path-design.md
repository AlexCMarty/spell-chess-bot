# Captures-only legal-move fast path for the freeze/jump rescan escape hatch — design

Date: 2026-09-05
Status: approved

## Goal

Resume the perf track paused at item 16 of `spell-chess-engine-followups` memory, but
not via its originally-scoped next step (moving the root generators onto the delta
path). A same-session spike (re-running `qprof`/`hotcost`/`bench` at depth 6 and 8 on
`main` at `6c8fc2a`) found the root generators unchanged and still ~0.6% of runtime —
confirming they're not where the remaining time goes — and instead surfaced that
`generate_quiescence_from`'s freeze branch is dominated by its own `NeedsRescan`
escape hatch: `FREEZE_RESCAN` costs 6.2s of 81.5s of quiescence-generation time at
depth 8 (7.6% of the bucketed total), nearly 3x `FREEZE_DELTA`'s 2.2s despite handling
fewer cases (`JUMP_RESCAN`'s equivalent cost is negligible, 0.7%).

A second, targeted measurement (temporary `qprof` counters, reverted after use — see
below) ruled out the design's original hypothesis and pointed at a different fix:

- Splitting `FREEZE_RESCAN`'s 142,947 calls (depth 6) by cause: 81.0% are "already in
  check, and the freeze zone touches a checker" (check dispelled or reduced — the case
  the existing code comment already flags as "essentially the whole position becomes
  legal, not a cheap delta"), 0.04% are "in check, freeze zone doesn't touch any
  checker" (the case this design originally hoped to delta cheaply — it barely exists,
  because `relevant_freeze_targets` already filters candidates toward the action), and
  19.0% are other decline reasons (pins, en passant, king-adjacent-king).
- So extending `spell_delta.rs`'s delta modeling further is not a good next lever: the
  dominant case is inherently expensive to delta (dispelling a check really does open
  most of the board), and the case that looked cheap to add barely occurs.
- Instead, the rescan itself is wasteful regardless of *why* it triggered: both
  `FREEZE_RESCAN` and `JUMP_RESCAN` (`crates/core/src/legal.rs:~660` and `~700`) call
  `legal_moves(&hypo)` — which builds full pseudo-legal move lists (quiet *and*
  capture) via `pseudo_legal_moves` — then immediately filter down to captures with
  `is_capture`. Quiescence never wants the quiet moves it just paid to generate.
  `hotcost` shows `pseudo_legal_moves` (670-890ns) as the dominant cost inside
  `legal_moves` (874-1200ns); a captures-only pseudo-legal generator skips that waste
  without touching any freeze/jump-specific legality reasoning at all.

This design scopes that fix: a `legal_captures(pos)` fast path, reusing `legal_moves`'
existing filters unchanged, replacing both rescan call sites.

## Why this is lower-risk than more delta modeling

Every prior perf item in this codebase that touched `spell_delta.rs` (items 13, 15)
had to reason about new freeze/jump interaction cases and found real bugs doing so (8
across those two items, all caught by hand-built geometry, none by random fuzzing —
see `fuzzing-is-not-discovery` memory). This design adds **no new legality reasoning**:
`legal_captures` must produce exactly the same answer `legal_moves` already produces,
just without constructing the quiet moves that get discarded. The correctness
obligation is "generate a subsequence of what `pseudo_legal_moves` would have
generated, filtered by the identical predicate" — a refactor-with-equivalence-proof,
not a new algorithm.

## Design

### `pseudo_legal_captures` (new, `crates/core/src/movegen.rs`)

Mirrors `pseudo_legal_moves`'s structure exactly — same `own.minus(frozen)` square
iteration order, same per-`PieceKind` dispatch — so that filtering its output for
captures is a subsequence of filtering `pseudo_legal_moves`'s output for captures
(same relative order, fewer entries). Differences per arm:

- **Knight / King:** intersect the attack pattern with `enemy_bb` (mirrors
  `spell_delta.rs`'s existing `piece_capture_targets` helper) instead of subtracting
  `own` — no separate own-piece exclusion needed since `enemy_bb` already excludes
  every own-colored square.
- **Bishop / Rook / Queen:** intersect slide-attacks with `enemy_bb` instead of
  `.minus(own)`.
- **Pawn:** keep only the diagonal-capture loop (including en passant); drop the
  single-push and double-push blocks entirely. This also correctly drops quiet
  promotions while keeping capture-promotions, with no separate promotion-vs-capture
  branch needed — a promotion is only emitted at all when the underlying move survives
  whichever loop produces it.
- **Castling:** dropped entirely — never a capture.

### Shared filter extraction (`crates/core/src/legal.rs`)

`legal_moves`'s current per-move closure (the six-filter enumeration already
documented and pinned by the `LEGAL_MOVE_FILTERS == 6` tripwire) is extracted into a
private `fn move_survives(pos: &Position, mv: &PieceMove, ctx: &LegalCtx) -> bool`,
where `LegalCtx` bundles what `legal_moves` currently computes inline: `mover`,
`enemy`, `king_sq`, `checker_count`, `checkers`, `pins`, `jump`. `legal_moves` and the
new `pub fn legal_captures(pos: &Position) -> Vec<PieceMove>` both build one `LegalCtx`
per call, then differ only in which generator (`pseudo_legal_moves` vs
`pseudo_legal_captures`) they filter through it. `legal_moves`'s public signature and
observable output are unchanged — this is a pure internal refactor, verified by a
before/after equivalence test (see Testing).

The `LEGAL_MOVE_FILTERS == 6` tripwire and its accompanying comment in `legal.rs`
move with the extracted function; no change to the count or the enumeration itself.

### Call-site change (`crates/core/src/legal.rs`, both rescan arms)

```rust
// before (both FREEZE_RESCAN and JUMP_RESCAN arms):
for mv in legal_moves(&position_with_field(pos, cast)) {
    if is_capture(pos, &mv) && !in_baseline(baseline, mv) {
        turns.push(Turn { spell: Some(cast), mv });
    }
}

// after:
for mv in legal_captures(&position_with_field(pos, cast)) {
    if !in_baseline(baseline, mv) {
        turns.push(Turn { spell: Some(cast), mv });
    }
}
```

No other call site changes. `legal_moves` itself, and every existing caller of it
(CLI, `game_status`, the root generators still on the rescan path per items 13/14/16),
is untouched — this only changes the two quiescence rescan arms.

## Testing

Per this project's standing discipline (`fuzzing-is-not-discovery` memory: random
batteries have never been the discovery method here; hand-built geometry and
mutation testing have found every real bug):

1. **Refactor-safety for `legal_moves`.** Before/after the extraction, assert
   `legal_moves(pos)` is byte-identical (same `Vec<PieceMove>`, same order) across the
   existing hand-built fixtures, `oracle_vectors.rs`'s fixtures, and a differential
   sweep. This is the guard against the one part of this change that touches the
   existing canonical function's internals.
2. **`legal_captures` equivalence, order-sensitive.** `legal_captures(pos)` must equal
   `legal_moves(pos).into_iter().filter(|mv| is_capture(pos, mv)).collect()` as an
   **ordered sequence**, not just a set — item 13's lesson applies directly here
   (`generate_quiescence_from` consumes emission order unsorted, so a same-set/
   different-order bug changes qnode counts and beta-cutoff behavior without changing
   correctness in the narrow sense). Run over the existing hand-built adversarial
   geometries already in `spell_delta_soundness.rs` and `oracle_vectors.rs` (pins,
   multi-check, stacked jump transparency, sparse endgames), not just random positions.
   Explicit fixtures required: en passant captures present; capture-promotions present
   and quiet promotions absent; a pinned piece capturing onto a live jump square (the
   clone-and-rescan arm inside `move_survives`'s pin filter); a pre-existing double
   check (only king captures should ever survive the filter, so
   `pseudo_legal_captures` must still emit the king's capture candidates for the
   filter to correctly accept/reject them).
3. **End-to-end invariance.** This is a pure optimization — nothing about which turns
   get generated should change anywhere in the tree. The release `--ignored`
   perf-bounds suite's node/qnode counters must come out bit-identical to the
   pre-change baseline at every measured depth (same gate items 13-16 used). Any
   deviation means the equivalence claim in point 2 has a gap.

Mutation-test each new/moved guard (`--no-fail-fast`, per item 13's standing process)
rather than trusting the differential batteries alone to prove a guard load-bearing.

## Measurement

No target number committed in advance — report whatever this actually gets, the way
item 14 and 15 did. Baseline captured this session (release, this Pi 5,
single-threaded, `bench` example, `main` at `6c8fc2a`): depth 6 = 6.845s, depth 8 =
96.256s (both within noise of memory's 6.876s/95.834s). Re-measure `bench` and `qprof`
at the same depths after implementation; node/qnode counts must stay bit-identical
(current baseline: d6 340473/2383276, d8 6453264/17437140).

Build via `cargo build --release --workspace --bins --examples` (not `--examples`
alone — see `perf-measure-the-binary-you-shipped` memory).

## Non-goals

- No further `spell_delta.rs` delta modeling for the freeze in-check case — ruled out
  above by measurement, not merely deferred.
- Root generators (`generate_turns_from`, `generate_search_spell_turns`) stay on the
  rescan path, unchanged, per items 13/14/16's standing decision. This design does not
  reopen that question.
- `is_capture` itself is unchanged and stays in use at any call site that still needs
  a boolean capture test on an already-generated move (e.g. checking a baseline move);
  only the two rescan arms that generate-then-filter change.
- No change to `pseudo_legal_moves` or `legal_moves`'s generation logic beyond the
  closure extraction — this is not an opportunity to also revisit filter correctness.

## Workflow

Branch off `main` (`6c8fc2a` at time of writing). This spec is written and committed
before implementation begins, per the architectural brainstorming path. Single merge
at the end (`--no-ff`, feature branch deleted after), matching items 13-15's pattern.

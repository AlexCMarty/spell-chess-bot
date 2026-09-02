# Spell-enabled capture discovery by delta, not rescan — design

Date: 2026-09-01
Status: approved

## Goal

Cut the per-node cost of spell-enabled capture discovery, which currently dominates
search time. Measured on this machine (Pi 5, release build, `go --depth N` on the
starting position):

| search | current |
|---|---|
| depth 6 | 13.5s (340k nodes, 2.38M qnodes, 25k nps) |
| depth 8 | 258s (6.45M nodes, 17.4M qnodes, 25k nps) |
| depth 10 | not measured — still running when the harness cut it off at ~5min |

The cost is concentrated in one function. Per-call microbenchmarks, starting position
and six plies in:

| function | ns/call | called |
|---|---|---|
| `hash_position` | 50 | every alphabeta node |
| `evaluate` | 86 | every node |
| `is_square_attacked` | 27 | every node |
| `apply_turn` | 74 | every child |
| `legal_moves` | 911–1,349 | every node, **and once per rescan** |
| `generate_quiescence_recapture_turns` | 648–696 | recursive q-nodes |
| **`generate_quiescence_turns_from`** | **35,981–41,446** | **every leaf node** |
| `generate_search_turns` | 113,645–201,095 | root / fallback |

`generate_quiescence_from` (`legal.rs:563`) calls
`legal_moves(&position_with_field(pos, cast))` once per relevant jump target
(`legal.rs:589`) and once per relevant freeze target (`legal.rs:606`) — roughly 30 full
legal-move generations per leaf at ~1.2µs each — then discards everything except
captures not already in `baseline`. The engine spends most of its life building
complete move lists for hypothetical positions and throwing away ~95% of each.

This is the follow-up the 2026-08-13 bitboard spec named and deferred: *"No this-ply
freeze/jump reuse filter in this pass. `generate_turns_from` still calls `legal_moves`
once per relevant spell target. If measurement misses the depth-3 bar, that filter is
the explicit fallback."* That "one `legal_moves` per spell target" pattern is shared by
`generate_turns_from` and `generate_quiescence_from`; it is the quiescence copy that
turned out to be the binding constraint, for the call-frequency reasons in Call sites.

## The constraint that shapes the design

An ablation confirmed the cost but also ruled out the obvious fix. Replacing the
leaf-node call with the cheap `generate_quiescence_recapture_turns` made depth 6
**3.4x faster** (13.5s → 3.9s, 25k → 96k nps) but made depth 8 **slower** (>400s vs
258s): the spell-enabled captures feed real beta cutoffs, so dropping them costs more
search than it saves generation.

**The target is therefore the same answer computed directly, not a smaller answer.**
The turn set must be unchanged.

## What the rules permit

Two rule facts bound the work, both `[VERIFIED]` in the corpus:

- **Jump affects only sliders.** `rules/40-jump.md` — "Knight: No — already ignores
  intervening pieces. King: No — moves one square." Pawns benefit only for the
  double-step, which is never a capture. So a jump can hand *us* a new capture only
  through one of our own sliders whose ray passes through the jumped square.
- **Freeze never changes reachability.** `rules/30-freeze.md` — frozen pieces "exert
  no control" but "still block sliding pieces." Freeze only removes enemy control, so
  it can enable a capture for us in exactly three ways: dispelling a check, removing a
  pinner, or removing the last defender of a piece our king can then take.

Freezing our *own* pieces only removes moves, never adds them; that case is already
handled cheaply by `move_survives_own_freeze` and is out of scope here.

## Non-goals

- **No search-algorithm changes.** TT, killers, history, ordering, pruning, quiescence
  depth and the `full_spells` gating in `search.rs:373` all stay exactly as they are.
  This pass changes only how a turn list is computed, never which turns are searched.
  In particular the tree is *deliberately* wide: the root always pairs the full
  freeze/jump set (needed for freeze-then-take at the root) and quiescence's freeze
  branch is no longer gated behind check/pin (needed for freeze-the-recapturer). Both
  came from `b8a262d`'s correctness fixes. Narrowing either is a tactic-visibility
  regression, not an optimisation.
- **No Lazy-SMP / multithreading.** Separately scoped; deliberately deferred until the
  single-thread node cost is fixed, since the two compound and parallelism would
  otherwise lock in the current per-node cost. `Position` is `Copy` with no heap
  (`FieldSet` is `[Option<SpellField>; 4]`), so nothing here forecloses it.
- **No incremental Zobrist, no move-buffer reuse.** Measured at ~2% combined. Not
  worth the complexity or the aliasing risk until this is done.
- **No eval changes.** `evaluate` at 86ns is not a bottleneck.
- **No rewrite of the root generators.** `generate_turns_from` and
  `generate_search_spell_turns` stay on the rescan path this pass — 0.6% of runtime and
  the harder semantics. See Call sites for the full reasoning.
- **No removal of the rescan implementation.** It is retained permanently as the
  differential-test oracle and as the runtime fallback.

## Architecture

A new `crates/core/src/spell_delta.rs` exposing one primitive:

```rust
/// Whether the fast path could settle the question for this cast.
pub enum Delta {
    /// `out` holds exactly the newly-legal captures. Authoritative.
    Complete,
    /// The fast path declined; the caller must run the rescan. `out` is untouched.
    NeedsRescan,
}

/// Captures that `cast` newly makes legal for `pos.side_to_move`, excluding
/// anything already legal in `baseline`. Appends to `out`.
pub fn captures_enabled_by(
    pos: &Position,
    cast: SpellCast,
    baseline: &[PieceMove],
    out: &mut Vec<PieceMove>,
) -> Delta;
```

`NeedsRescan` is the safety valve. For cases the fast path cannot settle cheaply, it
declines and the caller falls back to today's
`legal_moves(&position_with_field(pos, cast))`. **Correctness never depends on the fast
path being exhaustive — only speed does.** This lets the fast path ship handling the
common cases and widen later without ever risking a rules regression, and it means a
bug in the fast path degrades to slow-and-correct rather than fast-and-wrong.

The call site becomes:

```rust
let mut new_caps = Vec::new();
match spell_delta::captures_enabled_by(pos, cast, baseline, &mut new_caps) {
    Delta::Complete => { /* use new_caps */ }
    Delta::NeedsRescan => { /* existing legal_moves(&position_with_field(..)) path */ }
}
```

## The jump delta

For `cast.kind == Jump` on square `S`:

1. Our sliders that already reach `S`: `attackers_to(pos, S, us, frozen, slider_occ)`
   intersected with the bishop/rook/queen bitboards. This call already exists and is
   already used by `jump_may_change_this_ply` (`legal.rs:309`).
2. For each such slider `P`, continue its ray in the `P → S` direction past `S` to the
   next occupied square under `slider_occ`. If that square holds an enemy piece, `P`
   takes it: a candidate.
3. Discard candidates already in `baseline`. An enemy piece standing *on* `S` is
   already capturable without the spell (transparency is additive per
   `rules/40-jump.md`), so it is never new.
4. Legality-filter each surviving candidate:
   - if `P` is pinned, the destination must lie on `P`'s pin ray;
   - if we are in check, the capture must resolve it (capture the sole checker, or
     block on a non-jump square);
   - the move must not leave our king attacked **under the jump field** — jump is
     symmetric, so it can open an enemy slider onto our king. One
     `attackers_to`-style query per candidate at ~27ns.
5. Return `Complete`. Jump has no case the fast path cannot handle: the rules confine
   its effect to slider line-of-sight, which step 1 enumerates exhaustively.

Cost: a handful of ray operations plus ~27ns per surviving candidate, replacing a
1.2µs full rescan per target.

## The freeze delta

`freeze_enemy_affects_this_ply` (`legal.rs:255`) already enumerates the mechanisms, as
a boolean gate before a rescan: checkers intersecting the newly-frozen set, pinned
pieces whose pin ray is hit, attackers of king-adjacent squares, and attackers of
castle-transit squares. The first three are exactly the three capture-enabling
mechanisms the rules allow; the fourth governs castling legality only. The
decomposition is correct and rules-verified, so this design harvests it instead of
discarding it.

Each branch changes from returning `true` to emitting the captures it accounts for:

- **Pin released** — the pinner is newly frozen, so the piece it pinned is free. Emit
  that piece's captures, generated for that one piece only.
- **Defender removed** — an enemy piece adjacent to our king loses its last unfrozen
  defender. Emit the king capture.
- **Check dispelled, single checker** — the sole checker is newly frozen, so every
  move previously suppressed by the check becomes legal. Emit `NeedsRescan`: the newly
  legal set is essentially the whole position and is not a cheap delta. This case is
  rare (it needs us to be in check *and* hold a castable freeze) and is exactly what
  the escape hatch exists for.
- **Multi-checker, or a live en-passant square** — `NeedsRescan`. En passant is a
  capture, so it cannot simply be ignored; it is the trigger `generate_turns_from`
  already special-cases at `legal.rs:493`.
- **Castle-transit** — *ignored, not `NeedsRescan`*. This branch of
  `freeze_enemy_affects_this_ply` exists to decide whether castling stays legal, and a
  castle is never a capture, so it cannot contribute to this primitive's output.
  Declining on it would cost the fast path its most common freeze case for no
  correctness gain. The differential fuzz is what confirms this reasoning.

Freezing our own pieces is not handled here; `move_survives_own_freeze` already covers
it and stays where it is.

## Call sites

Only one call site is rewritten in this pass:

- `generate_quiescence_from` (`legal.rs:563`) — the 38µs leaf path, the whole point.

**The root generators are deliberately excluded, correcting an earlier scoping call.**
`generate_turns_from` (`legal.rs:450`) and `generate_search_spell_turns`
(`legal.rs:672`) look expensive per call (114–201µs) but are called rarely: the
`SPELLCHESS_PROFILE` counter reports `spell_gens=535` at depth 6, so they account for
535 × ~150µs ≈ 0.08s of a 13.5s search — **0.6% of runtime**. The per-call cost was
real; multiplying it by call frequency is the step that was missing.

They are also the harder case. `generate_quiescence_from` only ever *adds* captures
(it filters `is_capture(..) && !in_baseline(..)`), whereas `generate_turns_from` feeds
the full `legal_moves(&hypothetical)` list into `emit_spell_turns`, so it must also
account for baseline moves the field makes **illegal** — a jump opening an enemy slider
onto our own king, or an interposition square going transparent. Computing removals is
strictly more delicate than computing additions, for 0.6% of the runtime.

`spell_delta` is nonetheless designed so they can adopt it later without redesign: the
ray-extension and freeze-mechanism helpers are the reusable parts, and a future
`moves_under_field` entry point can layer removals on top of the same internals. That
is a follow-up, explicitly not this pass.

### Do not "fix" the exhaustive jump target list

`generate_quiescence_from:584` iterates the **exhaustive** `jump_targets` rather than
the relevance-filtered `relevant_jump_targets`, which looks like an obvious free win —
the latter is a proven strict subset, with a test asserting it
(`relevant_jump_targets_is_a_subset_of_jump_targets`).

**It is a known dead end. It was tried on 2026-08-22 and reverted: 13.4s → 18.0s at
depth 6, reproduced twice.** `relevant_jump_targets` runs `apply_move_only` plus a
relevance scan per baseline move (`spells.rs:207-210`), which costs more upfront than
it saves when called once per quiescence *entry* rather than once per recursive node.
The "relevance-filtered is always cheaper" intuition from the main search generator
does not transfer to entry-only call sites.

The delta rewrite makes this moot: it removes the per-target rescan that made the
target-list length matter in the first place. Do not reintroduce the swap without
fresh measurement.

## Testing

Ordered strongest-first; the first two are what make this safe.

1. **Differential fuzz against the rescan oracle.** For randomly generated positions —
   including live freeze/jump fields, checks, pins, and near-empty endgames — assert
   that when the fast path returns `Complete`, its output equals the rescan's output
   *as a set*. Follows the existing pattern from `ef67b4e`
   (`crates/core/tests/legal_moves_soundness.rs`). Runs over thousands of positions.
   Because `NeedsRescan` falls through to the oracle, only `Complete` needs checking.
2. **Search identity.** The turn set is unchanged and the search is deterministic, so
   `search()` at a fixed depth must return a **bit-identical move and score** to
   today's on a suite of positions. Any behavioural drift is a bug by construction.
   Capture today's outputs as fixtures before touching anything.
3. **Existing suites unchanged.** `oracle_vectors.rs`, `relevance_soundness.rs` and the
   full workspace test run must stay green with no assertion edits. Editing an existing
   rules assertion to make this pass is a defect, not a fix.
4. **Bench harness, committed.** Reports ns/call for each generator plus a `go --depth`
   ladder, so the win is measured rather than asserted. Replaces the throwaway probe
   used to produce the table above.
5. **Honest perf guard.** `depth_budget_stays_bounded_on_a_realistic_board`
   (`search.rs:730`) asserts depth-15 under 15s, is `#[ignore]`d, and has been silently
   false by roughly two orders of magnitude since `55b3f3e`. Replace its bounds with
   measured post-change numbers plus headroom, and say in the doc comment what machine
   produced them.

**Landing requirement.** Any change to `generate_quiescence_from` /
`generate_quiescence_turns_from` must be re-run against the release ignored test
(`cargo test -p spellchess-search --release -- --ignored`) *before* landing. The item-9
quiescence regression sat unnoticed on `main` for a full commit specifically because
that step was skipped.

## Measurement

Success is identical search results at materially lower cost:

| search | current | target | achieved (2026-09-01, post spell-delta rewrite) |
|---|---|---|---|
| depth 6 | 13.5s | ≤4s | 8.707s (340,473 nodes, 2,383,276 qnodes, 39,105 nps — node/qnode counts bit-identical to baseline) |
| depth 8 | 258s | ≤90s | 143.536s (6,453,264 nodes, 17,437,140 qnodes, 44,959 nps — node/qnode counts bit-identical to baseline) |
| `generate_quiescence_turns_from` | 36–41µs | ≤4µs | 15,501ns (starting position) / 17,702ns (6 plies in) |

Both time targets were **missed**. Depth 6 landed at 8.7s against a 4s target (roughly
2.2x over); depth 8 landed at 143.5s against a 90s target (roughly 1.6x over) — real
wins (13.5s→8.7s and 258s→143.5s), just short of the spec's aspiration. Node and qnode
counts are exactly unchanged from the pre-rewrite baseline, confirming the searched tree
is untouched and the gap is pure per-call cost, not a change in what's searched.
`generate_quiescence_turns_from` itself is the named remaining hot spot: it dropped
2.3x (35,981ns → 15,501ns on the starting position) but is still ~3.9x over its own
≤4µs target and remains the dominant per-leaf-node cost (called once per regular-search
leaf node, not per qnode, per the item-9-follow-up fix that moved recursive quiescence
nodes onto the cheaper `generate_quiescence_recapture_turns` path). Closing the
remaining gap would mean cutting further into `generate_quiescence_turns_from` itself —
out of scope for this task, which is measurement and honest guards, not a further
optimization pass.

The 3.4x from the ablation is the floor, not the ceiling: that experiment deleted the
work, whereas this accelerates it, so node counts stay at the (lower, better-pruned)
baseline rather than the ablation's inflated ones.

If measurement lands short of these, the explicit fallback is to narrow the
`NeedsRescan` surface — measure which branch is declining most often and give it a
real delta — rather than to start dropping turns from the search.

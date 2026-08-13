# Search branching-factor reduction — design

Date: 2026-08-12
Status: approved

## Goal

`crates/core/src/spells.rs`'s `freeze_targets`/`jump_targets` enumerate all 64 squares /
all occupied squares unconditionally, with no filtering for whether a target actually
changes anything. `generate_turns` pays a full `legal_moves()` recompute (pseudo-legal
movegen + check-filter) for every one of those ~97 candidates (1 no-spell + up to 64
freeze + up to 32 jump), and this cost is paid at *every* search node, including inside
quiescence recursion — not just the root. That multiplier is what made `go --depth 1`
measure ~54s and a reviewer's alpha-beta prototype measure depth 2 at ~85s on a realistic
board (see project memory `spell-chess-engine-followups`).

Reduce this branching factor for `search` specifically, without weakening tactical
strength (the engine must still find jump-enabled captures, freeze-based check evasion,
etc. — the existing `finds_king_capture_via_jump_in_one` and jump-threat-eval tests are
the bar) and without changing any player/CLI-facing rules behavior.

## Non-goals

- No change to `generate_turns`'s exhaustive behavior — it remains the correctness
  oracle used by the CLI (notation resolution: a human must still be able to legally cast
  a spell that happens to have zero board effect) and by `terminal.rs` (checkmate/
  stalemate detection).
- No incremental/delta-based legality tracking (recomputing only the affected pieces
  instead of full `legal_moves()` per candidate). That's a larger, higher-risk rewrite of
  `attacks.rs`/`movegen.rs` internals; out of scope here, worth revisiting later if this
  fix isn't sufficient once measured.
- No depth-aware suppression of spell generation (skip casting below some search depth).
  Considered and rejected: unsound, can hide real tactics that depend on a spell cast
  appearing several plies deep, which contradicts the project's existing tactical test
  coverage.

## Approach

A freeze cast is a no-op — produces a legal-move set identical to the no-spell turn, just
with a spell resource spent — unless its 3×3 zone touches a square that is either
currently occupied or reachable by one of the mover's own legal moves this turn
(including a castling move's rook-landing square). Every consumer of frozen-zone state
(`attacks.rs`'s attack generation, `movegen.rs`'s pseudo-legal generation and castling
checks) only ever reads occupancy at generation time, and a freeze field persists for
exactly the caster's own move plus the opponent's one reply (`expires_after_ply: pos.ply +
1`) — so if the zone is empty now and nothing can land there this turn, it stays empty and
inert through the field's entire lifetime, since nothing else moves before it expires.

A jump cast only matters if the target square is relevant to the ray-walking that
`rays.rs` and `movegen.rs`'s pawn double-step logic do — the only places
`is_square_jump_active` is consulted besides `spells.rs` itself. This turned out to need
more conditions than the "first blocker on a ray" intuition it started from; each of the
following was found missing by differential fuzzing (random legal walks, ~63k positions
checked) against the exhaustive baseline before landing on the final rule:

- The target is the first blocker (ignoring jump transparency) on some slider's ray, from
  any rook/bishop/queen of either color — the original intuition, still the core case.
- The target is the *second* blocker on such a ray, i.e. the first blocker behind the
  first one. The field survives the caster's move plus the opponent's one reply, so at
  most one more piece (the opponent's own moving piece) can vacate the first blocker
  within the field's lifetime, exposing whatever sits behind it to a discovered attack
  through the jump-transparent target.
- The target is itself occupied by a slider. `is_square_attacked` detects a slider
  attacker by walking a ray *from the target square being checked* and taking whatever it
  finds last — the same `walk_ray` used for movement. If the slider itself sits on a
  jump-active square, that walk sees straight through it and reports whatever (if
  anything) is behind it instead, hiding the slider as an attacker.
- The target already carries an active jump field. Recasting there doesn't change any ray
  geometry, but it refreshes the field's expiry, extending its lifetime by a ply — which
  can matter for the opponent's very next reply even though nothing about the board
  changed.
- The target is the "mid" square for some pawn's double-step (either color, on that
  pawn's start rank) — unaffected by the above, a single direct occupancy check, no
  second-level case (a pawn's double-step isn't a multi-square ray).

The first three are checked on the current board *and* on the board as it would look after
each of the mover's own candidate moves this turn, for the same reason freeze's landing set
does: a slider arriving at (or vacating) a square can create a relevant relationship that
didn't exist before the move.

This filter is validated by differential property testing against the existing exhaustive
`generate_turns` as oracle rather than by proof alone — see Testing. The by-hand soundness
argument this section started with proved unreliable in practice (see the git history on
`crates/core/src/spells.rs` for the three fixes fuzzing found); treat the bullet list above
as the load-bearing spec, and the test suite as the actual guarantee.

### Why this is scoped to `search`, not `generate_turns` itself

The filter drops targets that are legal per the game's rules but have zero board effect
(a human could legally choose to "waste" a spell this way). `generate_turns` is the
API the CLI uses to resolve typed notation to a `Turn` — pruning inert-but-legal targets
from it would make the CLI silently refuse a legal player action. Since only `search`
needs candidates ranked/filtered for performance, the filtered generator is a separate,
additive function; `generate_turns` and its callers (CLI, `terminal.rs`) are untouched.

## API

New functions in `crates/core/src/spells.rs`, alongside the existing `freeze_targets`/
`jump_targets`:

```rust
pub fn relevant_freeze_targets(pos: &Position, color: Color, baseline: &[PieceMove]) -> Vec<Square>
pub fn relevant_jump_targets(pos: &Position, color: Color, baseline: &[PieceMove]) -> Vec<Square>
```

- `relevant_freeze_targets` takes the already-computed no-spell legal moves (`baseline`)
  rather than recomputing them. It builds a landing set = currently-occupied squares ∪
  each baseline move's `to` square ∪ each castling move's rook-landing square, then
  returns `freeze_zone(s)` unioned over every square `s` in that landing set. (Freeze
  zones are symmetric under the Chebyshev-distance-1 neighborhood, so "target's zone
  contains a landing square" is equivalent to "target is in that landing square's own
  zone" — one pass over the landing set suffices, not 64 zone-containment checks.) Still
  gated by `SpellCounter::castable()` exactly like `freeze_targets`.
- `relevant_jump_targets` also takes `baseline`, for the current-board-plus-hypothetical
  scanning described above. It walks each slider's rays with `rays::walk_ray`, marking the
  first and second blocker on each; marks every slider's own square; marks pawn
  double-step mid squares; and separately marks any square that already carries an active
  `is_square_jump_active` field. Every marked square is filtered to ones occupied in the
  *original* (pre-move) position, since that's the only domain `jump_targets` is ever
  defined over — a square only occupied in a post-move hypothetical (e.g. a move's `to`
  square) is never a valid cast target regardless of what the hypothetical scan finds.

New function in `crates/core/src/legal.rs`:

```rust
pub fn generate_search_turns(pos: &Position) -> Vec<Turn>
```

The existing clone-field-then-`legal_moves` loop in `generate_turns` is factored into a
private `generate_turns_from(pos, freeze_targets: Vec<Square>, jump_targets: Vec<Square>)
-> Vec<Turn>` helper shared by both `generate_turns` (passes the exhaustive target lists)
and `generate_search_turns` (passes the relevance-filtered lists). `generate_search_turns`
computes `legal_moves(pos)` once and reuses it as both the no-spell baseline turns and the
input to `relevant_freeze_targets`, instead of the current code's ~97 implicit
`legal_moves` calls.

`generate_turns`'s public signature and behavior are unchanged.

## `search` integration

Swap the production call sites in `crates/search/src/search.rs` (root move generation,
quiescence recursion, move ordering input) from `generate_turns` to
`generate_search_turns`.

Leave the sanity-check call site at `search.rs:352`
(`generate_turns(&pos).contains(&turn)`) pointed at the exhaustive `generate_turns` — it
verifies the search's final chosen move is legal per ground truth, independent of whether
`generate_search_turns` has a bug, and is exactly the kind of regression guard this change
should keep. Test-only call sites in `ordering.rs` and `zobrist.rs` are not performance
paths and can stay on `generate_turns`.

## Testing

Two property tests carry the correctness weight, run over a battery of positions: the 20
existing oracle-vector positions (`crates/core/src/legal.rs` tests), a handful of
hand-authored dense/sparse/edge positions, and randomly-generated legal positions produced
by a random legal-move walk from the starting position.

1. **This-ply soundness.** For every target `Z` the filter excludes, assert
   `legal_moves(pos) == legal_moves(position_with_field(pos, Z))` as sets — an excluded
   target must not change the mover's own legal moves this turn.
2. **Next-ply soundness.** For every excluded `Z` and every baseline move `mv`, apply `mv`
   both with and without `Z`'s field active, then assert the resulting positions'
   `legal_moves()` (from whoever moves next) are identical. This is the one that would
   catch a field that's still active when the opponent's reply lands a piece inside its
   zone — the subtle case that ruled out a naive "just check current occupancy" filter for
   freeze during the design discussion.

These live in a new `crates/core/tests/relevance_soundness.rs` integration test, since
they exercise `generate_turns` and `generate_search_turns` together as oracle vs.
candidate.

Supporting tests:

- `relevant_freeze_targets(..) ⊆ freeze_targets(..)` and `relevant_jump_targets(..) ⊆
  jump_targets(..)` — the filter only removes candidates, never invents one.
- Concrete reduction assertions demonstrating the win: a general `relevant.len() <
  exhaustive.len()` check on the starting position (not a hardcoded magic number — the
  exact count is an implementation detail to verify empirically, not something to pin down
  by hand in this doc), plus one hand-verified concrete example: in the starting position,
  `f1`'s bishop is never the first blocker on any slider's ray (the queen's rank-ray and
  h1's rook are each blocked earlier, by `e1`'s king and `g1`'s knight respectively), so
  `f1` is excluded from `relevant_jump_targets` while still present in the exhaustive
  `jump_targets`. Live alongside the new functions in `spells.rs`.
- A `search`-side perf test, following the existing pattern of
  `time_budget_is_respected_on_a_full_board`: fixed-depth search on a realistic,
  tactically-dense position (reconstructed for this test — the original reviewer
  benchmark position wasn't preserved) asserts completion within a concrete wall-clock
  bound, replacing today's implicit "depth is unbounded, untested" gap.

Development order (TDD): write the soundness and reduction tests first against a stub
`relevant_freeze_targets`/`relevant_jump_targets` that returns the full unfiltered set
(soundness trivially holds, reduction assertions fail), then implement the real filtering
and watch reduction tests go green while soundness tests keep passing.

No `rules/` documentation changes — `generate_turns`, the player/CLI-facing API, is
unchanged, so no player-observable rules behavior changes.

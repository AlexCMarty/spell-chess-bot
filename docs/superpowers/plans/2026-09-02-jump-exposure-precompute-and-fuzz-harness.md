# Jump-Exposure Precompute + Spell-Delta Fuzz Harness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `jump_captures`' per-jump-target `attackers_to` call with a per-node precompute of which squares could expose the king to a new checker, and add a `cargo-fuzz` harness over both spell deltas (freeze and jump) to validate it and provide a second, structurally different bug-discovery method alongside hand-built geometries.

**Architecture:** A new `JumpExposure` precompute (walks the king's 8 rook/bishop rays once per node in `NodeContext::new`, same style as the existing `SliderScan`) replaces a per-target `attackers_to` ray/leaper scan in `jump_captures`. A new `crates/core/fuzz/` cargo-fuzz crate with two libFuzzer targets (`fuzz_freeze`, `fuzz_jump`) decodes fuzzer bytes into adversarial positions (byte-driven port of the existing `king_tangle_positions` generator) and runs the same set-and-order differential check against `rescan_oracle` that the hand-written test suite already uses.

**Tech Stack:** Rust (stable for `spellchess-core`/workspace, nightly + `cargo-fuzz` 0.13 for the fuzz crate only), existing `Bitboard`/`rays` primitives, libFuzzer via `libfuzzer-sys`.

**Spec:** `docs/superpowers/specs/2026-09-02-jump-exposure-precompute-and-fuzz-harness-design.md`

## Global Constraints

- Node counts at depth 6 (340473 nodes / 2383276 qnodes) and depth 8 (6453264 / 17437140) must stay bit-identical to the current `main` baseline (`1b93e07`) — this precompute must not change the search tree, only its cost.
- Every new guard added to the precompute (frozen check, color check, piece-kind check) must be mutation-tested with `--no-fail-fast` before the task is considered done — deleting the guard must make a specific named test fail.
- Every hand-built test fixture must include an assertion against the ground-truth oracle (`rescan_oracle` or a direct `legal_moves` call on the hypothetical position) proving the fixture actually exercises the geometry claimed — a fixture whose oracle assertion isn't checked first is vacuous (see `fuzzing-is-not-discovery` memory).
- `crates/core/fuzz/{Cargo.toml,fuzz_targets/}` are committed; `crates/core/fuzz/{corpus,artifacts}/` are gitignored (its `target/` is already covered by the repo's existing top-level `target/` ignore rule, which matches at any depth).
- No CI wiring, no fuzzing outside `spell_delta.rs`'s freeze/jump functions — both explicitly out of scope per the spec.
- Use `~/.cargo/bin/cargo` (plain `cargo` is not on `PATH` in this environment); the fuzz crate specifically needs `~/.cargo/bin/cargo +nightly fuzz ...`.

---

## Task 1: Branch setup + black-box characterization fixtures for jump exposure

**Files:**
- Modify: `crates/core/src/spell_delta.rs` (adds 7 new `#[test]` fns inside the existing `mod tests` block, which starts at line 562; add them after the last existing test)

**Interfaces:**
- Consumes: `assert_delta_sound(pos: &Position, cast: SpellCast) -> Delta`, `rescan_oracle(pos: &Position, cast: SpellCast, baseline: &[PieceMove]) -> Vec<PieceMove>`, `sq(s: &str) -> Square`, `put(pos: &mut Position, s: &str, color: Color, kind: PieceKind)`, `empty_board() -> Position` — all already defined at the top of `mod tests` (`spell_delta.rs:569-621`).
- Produces: nothing new consumed by later tasks — these are a regression net for Task 2's refactor. They must all **already pass against the current, unmodified `jump_captures`** (which is already correct; this task is not a bug fix) before Task 2 touches anything.

These fixtures characterize every branch of the jump-exposure rule in `docs/superpowers/specs/2026-09-02-jump-exposure-precompute-and-fuzz-harness-design.md`'s "Testing" section, purely through the existing public/test-module surface (no reference to types Task 2 will add yet).

- [ ] **Step 1: Create the feature branch**

```bash
cd "/home/alex/Spell Chess"
git checkout -b perf/jump-exposure-and-fuzz-harness main
```

- [ ] **Step 2: Add the seven characterization tests**

Insert at the end of `mod tests` in `crates/core/src/spell_delta.rs` (i.e. just before the closing `}` of the module, after the last existing `#[test]` fn):

```rust
    // -----------------------------------------------------------------------
    // Jump-exposure precompute characterization fixtures. These pin the rule
    // from the design doc *before* the precompute exists (Task 2 of the
    // implementation plan): every one of these must already pass against the
    // pre-precompute `jump_captures`, which computes the same answer via a
    // fresh `attackers_to` call per target. They exist to catch a regression
    // in the refactor, not to fix a bug.
    // -----------------------------------------------------------------------

    /// King d1's file ray is blocked by an enemy knight on d4 (blocker color is
    /// irrelevant to jump transparency), with a rook behind it on d8 -- jumping
    /// d4 must reveal exactly that rook as a checker. White's bishop on a1
    /// independently gains Nf6 through the same jump (its own a1-h8 diagonal
    /// also passes through d4), which is unrelated to the check and must be
    /// filtered: only a capture of the checker (d8) is a legal response to
    /// being in check.
    #[test]
    fn jump_exposure_on_a_rook_line_filters_captures_to_the_new_checker() {
        let mut pos = empty_board();
        put(&mut pos, "d1", Color::White, PieceKind::King);
        put(&mut pos, "d4", Color::Black, PieceKind::Knight);
        put(&mut pos, "d8", Color::Black, PieceKind::Rook);
        put(&mut pos, "a1", Color::White, PieceKind::Bishop);
        put(&mut pos, "f6", Color::Black, PieceKind::Knight);
        put(&mut pos, "h8", Color::Black, PieceKind::King);

        let baseline = legal_moves(&pos);
        let cast = SpellCast { kind: SpellKind::Jump, square: sq("d4") };
        let bxf6 = PieceMove::quiet(sq("a1"), sq("f6"));
        assert!(
            !rescan_oracle(&pos, cast, &baseline).contains(&bxf6),
            "fixture is wrong: rescan must filter Bxf6 once jump@d4 lets Rd8 check Kd1",
        );
        assert_delta_sound(&pos, cast);
    }

    /// Same mechanism, diagonal ray: king e1's (-1,+1) diagonal is blocked by a
    /// knight on c3 with a bishop behind it on a5. White's queen on e5
    /// independently gains Rxa1 through the same jump (its own diagonal through
    /// c3 in the *other* direction), which must be filtered the same way.
    #[test]
    fn jump_exposure_on_a_bishop_line_filters_captures_to_the_new_checker() {
        let mut pos = empty_board();
        put(&mut pos, "e1", Color::White, PieceKind::King);
        put(&mut pos, "c3", Color::Black, PieceKind::Knight);
        put(&mut pos, "a5", Color::Black, PieceKind::Bishop);
        put(&mut pos, "e5", Color::White, PieceKind::Queen);
        put(&mut pos, "a1", Color::Black, PieceKind::Rook);
        put(&mut pos, "h8", Color::Black, PieceKind::King);

        let baseline = legal_moves(&pos);
        let cast = SpellCast { kind: SpellKind::Jump, square: sq("c3") };
        let qxa1 = PieceMove::quiet(sq("e5"), sq("a1"));
        assert!(
            !rescan_oracle(&pos, cast, &baseline).contains(&qxa1),
            "fixture is wrong: rescan must filter Qxa1 once jump@c3 lets Ba5 check Ke1",
        );
        assert_delta_sound(&pos, cast);
    }

    /// King d1 is already in check from a knight on b2 (unrelated to any spell).
    /// jump@d4 additionally exposes Rd8 down the file -- turning a single check
    /// into a double check, where only king moves are legal. `jump_captures`
    /// must decline rather than reason about double check itself.
    #[test]
    fn jump_exposure_combined_with_a_pre_existing_checker_declines() {
        let mut pos = empty_board();
        put(&mut pos, "d1", Color::White, PieceKind::King);
        put(&mut pos, "b2", Color::Black, PieceKind::Knight);
        put(&mut pos, "d4", Color::Black, PieceKind::Knight);
        put(&mut pos, "d8", Color::Black, PieceKind::Rook);
        put(&mut pos, "h8", Color::Black, PieceKind::King);

        let cast = SpellCast { kind: SpellKind::Jump, square: sq("d4") };
        let hypothetical = crate::legal::position_with_field(&pos, cast);
        let hyp_moves = legal_moves(&hypothetical);
        assert!(
            !hyp_moves.is_empty() && hyp_moves.iter().all(|mv| mv.from == sq("d1")),
            "fixture is wrong: jump@d4 must leave only king moves (double check), got {hyp_moves:?}",
        );

        let baseline = legal_moves(&pos);
        let mut out = Vec::new();
        assert_eq!(
            captures_enabled_by(&pos, cast, &baseline, &mut out),
            Delta::NeedsRescan,
            "a jump that creates a double check must decline, not guess",
        );
        assert!(out.is_empty(), "a declining call must not touch `out`");
    }

    /// Same geometry as the rook-line fixture, but the piece behind the
    /// blocker is a Bishop -- the wrong kind for a straight-line ray. It must
    /// not be treated as a newly-revealed checker, so the unrelated Bxf6
    /// capture stays available.
    #[test]
    fn jump_exposure_requires_the_revealed_piece_kind_to_match_the_ray() {
        let mut pos = empty_board();
        put(&mut pos, "d1", Color::White, PieceKind::King);
        put(&mut pos, "d4", Color::Black, PieceKind::Knight);
        put(&mut pos, "d8", Color::Black, PieceKind::Bishop);
        put(&mut pos, "a1", Color::White, PieceKind::Bishop);
        put(&mut pos, "f6", Color::Black, PieceKind::Knight);
        put(&mut pos, "h8", Color::Black, PieceKind::King);

        let baseline = legal_moves(&pos);
        let cast = SpellCast { kind: SpellKind::Jump, square: sq("d4") };
        let bxf6 = PieceMove::quiet(sq("a1"), sq("f6"));
        assert!(
            rescan_oracle(&pos, cast, &baseline).contains(&bxf6),
            "fixture is wrong: a bishop on d8 cannot check Kd1 down the file, Bxf6 must stay legal",
        );
        assert_delta_sound(&pos, cast);
    }

    /// Same geometry as the rook-line fixture, but Rd8 is frozen by a
    /// pre-existing field. A frozen piece "exerts no control at all"
    /// (rules/30-freeze.md), so it must not be treated as a newly-revealed
    /// checker even though it is the right kind and color.
    #[test]
    fn a_frozen_revealed_piece_does_not_count_as_exposure() {
        let mut pos = empty_board();
        put(&mut pos, "d1", Color::White, PieceKind::King);
        put(&mut pos, "d4", Color::Black, PieceKind::Knight);
        put(&mut pos, "d8", Color::Black, PieceKind::Rook);
        put(&mut pos, "a1", Color::White, PieceKind::Bishop);
        put(&mut pos, "f6", Color::Black, PieceKind::Knight);
        put(&mut pos, "h8", Color::Black, PieceKind::King);
        add_field(&mut pos, "d8", Color::White, SpellKind::Freeze);

        let baseline = legal_moves(&pos);
        let cast = SpellCast { kind: SpellKind::Jump, square: sq("d4") };
        let bxf6 = PieceMove::quiet(sq("a1"), sq("f6"));
        assert!(
            rescan_oracle(&pos, cast, &baseline).contains(&bxf6),
            "fixture is wrong: a frozen Rd8 cannot check Kd1, Bxf6 must stay legal",
        );
        assert_delta_sound(&pos, cast);
    }

    /// Same geometry again, but Rd8 is White's own piece, not an enemy's. Only
    /// an enemy piece can check our king.
    #[test]
    fn an_own_colored_revealed_piece_does_not_count_as_exposure() {
        let mut pos = empty_board();
        put(&mut pos, "d1", Color::White, PieceKind::King);
        put(&mut pos, "d4", Color::Black, PieceKind::Knight);
        put(&mut pos, "d8", Color::White, PieceKind::Rook);
        put(&mut pos, "a1", Color::White, PieceKind::Bishop);
        put(&mut pos, "f6", Color::Black, PieceKind::Knight);
        put(&mut pos, "h8", Color::Black, PieceKind::King);

        let baseline = legal_moves(&pos);
        let cast = SpellCast { kind: SpellKind::Jump, square: sq("d4") };
        let bxf6 = PieceMove::quiet(sq("a1"), sq("f6"));
        assert!(
            rescan_oracle(&pos, cast, &baseline).contains(&bxf6),
            "fixture is wrong: White's own Rd8 cannot check Kd1, Bxf6 must stay legal",
        );
        assert_delta_sound(&pos, cast);
    }

    /// Same geometry with d8 empty entirely -- the king's ray runs off the
    /// board with only one blocker (d4) and nothing behind it. Must not panic
    /// and must not treat d4 as exposed.
    #[test]
    fn a_ray_with_no_second_blocker_creates_no_exposure() {
        let mut pos = empty_board();
        put(&mut pos, "d1", Color::White, PieceKind::King);
        put(&mut pos, "d4", Color::Black, PieceKind::Knight);
        put(&mut pos, "a1", Color::White, PieceKind::Bishop);
        put(&mut pos, "f6", Color::Black, PieceKind::Knight);
        put(&mut pos, "h8", Color::Black, PieceKind::King);

        let baseline = legal_moves(&pos);
        let cast = SpellCast { kind: SpellKind::Jump, square: sq("d4") };
        let bxf6 = PieceMove::quiet(sq("a1"), sq("f6"));
        assert!(
            rescan_oracle(&pos, cast, &baseline).contains(&bxf6),
            "fixture is wrong: with nothing behind d4, Bxf6 must stay legal",
        );
        assert_delta_sound(&pos, cast);
    }
```

- [ ] **Step 3: Run the new tests and confirm all seven pass against unmodified code**

```bash
~/.cargo/bin/cargo test -p spellchess-core --lib spell_delta::tests -- --no-fail-fast
```

Expected: all 7 new tests PASS (plus all pre-existing tests in the module), since `jump_captures` is not yet modified — this step confirms the fixtures are valid characterizations of *current* correct behavior, not a bug report.

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/spell_delta.rs
git commit -m "$(cat <<'EOF'
test(core): characterize the jump-exposure rule before the precompute refactor

Seven hand-built fixtures pin what jump_captures must keep doing once
attackers_to's per-target call is replaced by a per-node precompute:
rook-line and bishop-line exposure, combination with a pre-existing
checker, and the three guards (wrong kind, frozen, own-colored) plus
the off-board edge case. All pass against the unmodified implementation.
EOF
)"
```

---

## Task 2: Implement the `JumpExposure` precompute and wire it into `jump_captures`

**Files:**
- Modify: `crates/core/src/spell_delta.rs:64-125` (insert `JumpExposure` struct + `jump_exposure_scan` fn between `slider_scan` and `NodeContext`; extend `NodeContext` struct and `NodeContext::new`)
- Modify: `crates/core/src/spell_delta.rs:213-216` (only the `attackers_to` call and the comment above it inside `jump_captures` — the rest of the function, including the double-check decline logic that follows, is unchanged)
- Modify: `crates/core/src/spell_delta.rs` (add two white-box unit tests to `mod tests`: the exposure-scan check, and a regression fixture for a stacked-transparency bug found and fixed during this task's own execution — see the ruling before the `JumpExposure` code block below)

**Interfaces:**
- Consumes: `crate::rays::{ray_attacks, ROOK_DIRS, BISHOP_DIRS}` (all already `pub` in `rays.rs`), `Bitboard::{EMPTY, with, contains, intersect, iter, union}`, `Board::{get, occupancy}`.
- Produces: `NodeContext.jump_exposure: JumpExposure` (private field, `mod tests` can still reach it via `use super::*`), `JumpExposure::revealed_by(&self, s: Square) -> Bitboard` (empty when `s` isn't an exposure square — not `Option<Square>`, since a single blocker can reveal more than one attacker when jump fields are stacked) — Task 3's mutation testing targets the guards inside `jump_exposure_scan`.

- [ ] **Step 1: Add the white-box unit test first (will not compile yet)**

Add to `mod tests` in `crates/core/src/spell_delta.rs`, right after the 7 tests from Task 1:

```rust
    /// Direct check of the precompute itself, reusing the rook-line fixture's
    /// geometry: d4 must be recorded as exposing d8, and no other square on
    /// the board should be recorded as exposing anything.
    #[test]
    fn jump_exposure_scan_finds_exactly_the_blocker_and_its_revealed_attacker() {
        let mut pos = empty_board();
        put(&mut pos, "d1", Color::White, PieceKind::King);
        put(&mut pos, "d4", Color::Black, PieceKind::Knight);
        put(&mut pos, "d8", Color::Black, PieceKind::Rook);
        put(&mut pos, "h8", Color::Black, PieceKind::King);

        let ctx = NodeContext::new(&pos);
        assert_eq!(ctx.jump_exposure.revealed_by(sq("d4")), Bitboard::from_square(sq("d8")));
        assert_eq!(ctx.jump_exposure.mask.count(), 1, "only d4 should be recorded as exposed");
    }

    /// A pre-existing jump field on the REVEALED square itself: jumping g3
    /// (Black's own queen) opens the g-file toward White's rook on g5 -- but
    /// g5 already has its own live jump field, so it does not block anything
    /// and must still count as an attacker once revealed. Combined with a
    /// pre-existing checker (White's knight on h4, unrelated to any spell),
    /// jump@g3 must decline as a double check. This reproduces a real bug an
    /// earlier version of `jump_exposure_scan` had: it used `slider_occ`
    /// (which excludes g5, since g5 is already transparent) both to walk the
    /// ray *and* to test "is a piece here," so it was blind to any piece
    /// standing on an already-transparent square -- see the ruling in this
    /// plan's Task 2 for the fix.
    #[test]
    fn jump_exposure_sees_past_an_already_transparent_revealed_piece() {
        let mut pos = empty_board();
        put(&mut pos, "g2", Color::Black, PieceKind::King);
        put(&mut pos, "g3", Color::Black, PieceKind::Queen);
        put(&mut pos, "h4", Color::White, PieceKind::Knight);
        put(&mut pos, "g5", Color::White, PieceKind::Rook);
        put(&mut pos, "e1", Color::White, PieceKind::King);
        pos.side_to_move = Color::Black;
        add_field(&mut pos, "g5", Color::White, SpellKind::Jump);

        let ctx = NodeContext::new(&pos);
        assert_eq!(
            ctx.jump_exposure.revealed_by(sq("g3")),
            Bitboard::from_square(sq("g5")),
            "fixture is wrong: g5's rook must be found even though its own square is transparent",
        );

        let cast = SpellCast { kind: SpellKind::Jump, square: sq("g3") };
        let hypothetical = crate::legal::position_with_field(&pos, cast);
        let hyp_moves = legal_moves(&hypothetical);
        assert!(
            !hyp_moves.is_empty() && hyp_moves.iter().all(|mv| mv.from == sq("g2")),
            "fixture is wrong: jump@g3 must leave only king moves (double check from Nh4 and Rg5), got {hyp_moves:?}",
        );

        let baseline = legal_moves(&pos);
        let mut out = Vec::new();
        assert_eq!(
            captures_enabled_by(&pos, cast, &baseline, &mut out),
            Delta::NeedsRescan,
            "a jump that reveals a checker sitting on an already-transparent square must still decline",
        );
        assert!(out.is_empty(), "a declining call must not touch `out`");
    }
```

- [ ] **Step 2: Run it to verify it fails to compile**

```bash
~/.cargo/bin/cargo test -p spellchess-core --lib spell_delta::tests::jump_exposure_scan_finds_exactly_the_blocker_and_its_revealed_attacker
```

Expected: compile error, `no field \`jump_exposure\` on type \`&NodeContext\`` (or similar) — confirms the test exercises code that doesn't exist yet.

- [ ] **Step 3: Add the `JumpExposure` struct and `jump_exposure_scan` function**

Insert into `crates/core/src/spell_delta.rs` immediately after `slider_scan`'s closing `}` (currently ending at line 64) and before the `/// Everything the delta needs...` doc comment that precedes `NodeContext` (currently starting at line 66):

**Ruling (recorded 2026-09-02 during Task 2 execution — see the SDD ledger):**
the version below supersedes an earlier draft that used `slider_occ` to test
"is a piece on this square" when looking for the revealed attacker. That is
wrong: `slider_occ` excludes squares that are *already* jump-transparent from
an earlier field, so a piece standing on one of those squares was invisible
to the scan even though it still attacks from its own square (jump makes a
square transparent *to other rays passing through it*; it does not erase the
piece standing on it — see `attacks.rs`'s `slider_on_a_jump_square_still_attacks`
test for the same principle applied to `attackers_to`). The fix separates
"does this square block" (transparency-based) from "is there an attacker
here" (real board occupancy) and walks *through* any already-transparent
occupied square instead of stopping at it, checking each one along the way.
This also means a single blocker can now reveal more than one attacker (only
when multiple pre-existing jump fields are stacked on the same line, which is
rare), so `JumpExposure` stores a `Bitboard` of revealed squares per blocker
instead of a single `Square`.

```rust
/// Squares whose jump could add a new attacker on our king, precomputed once
/// per node (see docs/superpowers/specs/2026-09-02-jump-exposure-precompute-
/// and-fuzz-harness-design.md). Jump only grants slider transparency, so the
/// only way it can add a checker is: `blocker` is the first REAL (non-
/// transparent) piece on one of the king's 8 rook/bishop lines, and beyond it
/// -- skipping over any square that already has a live jump field of its own,
/// since those don't block either -- sits at least one matching, unfrozen
/// enemy slider. `revealed` is every such square found along that line
/// (usually zero or one; more than one only when multiple pre-existing jump
/// fields are stacked on the same line). At most 8 entries: a square lies on
/// at most one of the king's 8 lines.
struct JumpExposure {
    mask: Bitboard,
    count: usize,
    pairs: [(Square, Bitboard); 8],
}

impl JumpExposure {
    const EMPTY: JumpExposure =
        JumpExposure { mask: Bitboard::EMPTY, count: 0, pairs: [(Square(0), Bitboard::EMPTY); 8] };

    /// The attacker(s) a jump on `s` would reveal (empty if none).
    fn revealed_by(&self, s: Square) -> Bitboard {
        self.pairs[..self.count]
            .iter()
            .find(|&&(blocker, _)| blocker == s)
            .map(|&(_, r)| r)
            .unwrap_or(Bitboard::EMPTY)
    }
}

fn jump_exposure_scan(
    board: &Board,
    king_sq: Square,
    enemy: Color,
    frozen: Bitboard,
    slider_occ: Bitboard,
    jump: Bitboard,
) -> JumpExposure {
    let real_occ = board.occupancy();
    let mut exp = JumpExposure::EMPTY;
    for &dir in crate::rays::ROOK_DIRS.iter().chain(crate::rays::BISHOP_DIRS.iter()) {
        let Some(blocker) =
            crate::rays::ray_attacks(king_sq, slider_occ, dir).intersect(slider_occ).iter().next()
        else {
            continue;
        };
        let is_rook_dir = crate::rays::ROOK_DIRS.contains(&dir);
        let mut revealed = Bitboard::EMPTY;
        let mut from = blocker;
        loop {
            let Some(next) =
                crate::rays::ray_attacks(from, real_occ, dir).intersect(real_occ).iter().next()
            else {
                break;
            };
            let piece = board.get(next).expect("real_occ square must be occupied");
            let kind_matches = if is_rook_dir {
                matches!(piece.kind, PieceKind::Rook | PieceKind::Queen)
            } else {
                matches!(piece.kind, PieceKind::Bishop | PieceKind::Queen)
            };
            if piece.color == enemy && !frozen.contains(next) && kind_matches {
                revealed = revealed.with(next);
            }
            if !jump.contains(next) {
                break;
            }
            from = next;
        }
        if !revealed.is_empty() {
            exp.mask = exp.mask.with(blocker);
            exp.pairs[exp.count] = (blocker, revealed);
            exp.count += 1;
        }
    }
    exp
}
```

- [ ] **Step 4: Extend `NodeContext` to hold a `JumpExposure`**

In the `NodeContext` struct definition (`crates/core/src/spell_delta.rs`, currently lines 76-92), add a field after `sliders: SliderScan,`:

```rust
    sliders: SliderScan,
    jump_exposure: JumpExposure,
}
```

In `NodeContext::new` (currently lines 94-125), after the line `sliders: slider_scan(&pos.board, us, frozen, slider_occ),` inside the struct literal, the struct literal needs a new field. Replace the whole `NodeContext { ... }` struct-literal return (currently lines 112-123):

```rust
        let jump_exposure = match king_sq {
            Some(k) => jump_exposure_scan(&pos.board, k, enemy, frozen, slider_occ, jump),
            None => JumpExposure::EMPTY,
        };
        NodeContext {
            us,
            enemy,
            king_sq,
            frozen,
            jump,
            slider_occ,
            enemy_bb: pos.board.color_bb(enemy),
            checkers,
            pins,
            sliders: slider_scan(&pos.board, us, frozen, slider_occ),
            jump_exposure,
        }
```

- [ ] **Step 5: Replace the `attackers_to` call in `jump_captures`**

Replace this block (currently lines 213-216 — the rest of the `if checkers_after.count() > 1 { ... }` body that follows, and everything after it in the function, is unchanged):

```rust
    // Jump is symmetric: it can open an enemy slider onto our own king. Recompute
    // our king's check/pin context under the field rather than assuming it holds.
    let checkers_after = attackers_to(pos, king_sq, ctx.enemy, ctx.frozen, slider_occ_after);
    if checkers_after.count() > 1 {
```

with:

```rust
    // Jump is symmetric: it can open an enemy slider onto our own king.
    // `ctx.jump_exposure` was precomputed once for the whole node (see
    // `jump_exposure_scan`) instead of walking `attackers_to` fresh for every
    // target: checkers_after is provably `ctx.checkers` unchanged unless `s`
    // is one of the (at most 8) squares that precompute recorded, and
    // `revealed_by` returns an empty Bitboard (a no-op union) when it isn't.
    let checkers_after = ctx.checkers.union(ctx.jump_exposure.revealed_by(s));
    if checkers_after.count() > 1 {
```

Leave the rest of the function (the double-check decline, the `sliders.reach` early return, the pin walk, the capture loop) exactly as it is — this replaces only where `checkers_after` comes from, not the logic built on it.

- [ ] **Step 6: Run the white-box test and all of Task 1's fixtures**

```bash
~/.cargo/bin/cargo test -p spellchess-core --lib spell_delta::tests -- --no-fail-fast
```

Expected: all tests PASS, including the two new white-box tests, the stacked-transparency
regression fixture, and all 7 fixtures from Task 1.

- [ ] **Step 7: Run the full workspace test suite**

```bash
~/.cargo/bin/cargo test --workspace --release 2>&1 | tail -60
```

Expected: all tests pass (this also re-runs `spell_delta_soundness.rs`'s random/adversarial batteries against the new code path).

- [ ] **Step 8: Commit**

```bash
git add crates/core/src/spell_delta.rs
git commit -m "$(cat <<'EOF'
perf(core): precompute jump-exposure once per node instead of per target

jump_captures called attackers_to fresh for every jump target just to
check for a newly-created checker -- 7.22M calls at depth 6 alone. Jump
only grants slider transparency, so the only way it can add a checker
is a specific, boundable set of at-most-8 squares per node (first
blocker on one of the king's 8 rays, with at least one matching
unfrozen enemy slider behind it -- walking through any square that is
itself already jump-transparent from an earlier field, since those
don't block either). Precompute that set once in NodeContext::new and
reduce the per-target check to a bitboard lookup.

Includes a regression fixture for a stacked-transparency bug caught by
the existing spell_delta_soundness.rs battery during this task: a
revealed attacker sitting on its own already-transparent square was
invisible to an earlier version of the scan.
EOF
)"
```

---

## Task 3: Mutation-test the four new guards in `jump_exposure_scan`

**Files:**
- Modify (temporarily, one guard at a time, then revert): `crates/core/src/spell_delta.rs`

**Interfaces:**
- Consumes: the combined match condition added in Task 2
  (`if piece.color == enemy && !frozen.contains(next) && kind_matches { revealed = revealed.with(next); }`)
  and the walk-through-transparency guard (`if !jump.contains(next) { break; }`).
- Produces: confidence that each guard is load-bearing, per the project's standing mutation-testing requirement for freeze/jump legality changes.

- [ ] **Step 1: Mutate the frozen-check guard and confirm a named test dies**

Temporarily change:

```rust
            if piece.color == enemy && !frozen.contains(next) && kind_matches {
```

to:

```rust
            if piece.color == enemy && kind_matches {
```

Run:

```bash
~/.cargo/bin/cargo test -p spellchess-core --lib spell_delta::tests -- --no-fail-fast 2>&1 | grep -A3 "FAILED\|test result"
```

Expected: `a_frozen_revealed_piece_does_not_count_as_exposure` FAILS (and only that test, or that test plus others that happen to also exercise it — confirm the frozen fixture specifically is in the failure list). Revert afterward.

- [ ] **Step 2: Mutate the color-check guard and confirm a named test dies**

Temporarily change the same line to:

```rust
            if !frozen.contains(next) && kind_matches {
```

Run the same test command. Expected: `an_own_colored_revealed_piece_does_not_count_as_exposure` FAILS. Revert.

- [ ] **Step 3: Mutate the kind-match guard and confirm a named test dies**

Temporarily change the same line to:

```rust
            if piece.color == enemy && !frozen.contains(next) {
```

Run the same test command. Expected: `jump_exposure_requires_the_revealed_piece_kind_to_match_the_ray` FAILS. Revert.

- [ ] **Step 4: Mutate the walk-through-transparency guard and confirm a named test dies**

This is the mechanism that fixes the stacked-transparency bug found during this task — it must be load-bearing too. Temporarily change:

```rust
            if !jump.contains(next) {
                break;
            }
```

to:

```rust
            break;
```

Run the same test command. Expected: `jump_exposure_sees_past_an_already_transparent_revealed_piece` FAILS (it should find nothing at g3, or find the wrong thing, once the walk can never see past g5). Revert.

- [ ] **Step 5: Confirm the file is back to the Task 2 committed state and all tests pass**

```bash
git diff crates/core/src/spell_delta.rs
```

Expected: empty diff (all four mutations reverted). Then:

```bash
~/.cargo/bin/cargo test -p spellchess-core --lib spell_delta::tests -- --no-fail-fast
```

Expected: all tests pass again. No commit needed for this task — it's a verification pass, not a code change (the guards were already committed in Task 2).

---

## Task 4: Node-count regression gate and wall-clock measurement

**Files:** none modified — verification only.

**Interfaces:**
- Consumes: `crates/search/examples/bench.rs` (the committed perf-measurement binary — do not use the `spellchess` CLI, see `perf-measure-the-binary-you-shipped` memory), the release `--ignored` test suite.

- [ ] **Step 1: Run the release ignored perf suite and confirm it passes**

```bash
~/.cargo/bin/cargo test -p spellchess-search --release -- --ignored
```

Expected: PASS (depths 1/3/4/6/8 all under their bound multipliers — this is the standing gate from item 13/14, not new for this task).

- [ ] **Step 2: Measure node counts and wall-clock with `bench`, before and after**

`bench.rs` takes the depth as its one positional arg (default 6), reads thread count from
`SPELLCHESS_THREADS` (default: every core), and `dump_profile` in `search.rs` prints
`nodes=`/`qnodes=` to stderr when `SPELLCHESS_PROFILE` is set. Force single-threaded so the
timing is directly comparable to item 14's numbers (Lazy-SMP timings vary run to run):

```bash
~/.cargo/bin/cargo build --release --workspace --bins --examples
SPELLCHESS_THREADS=1 SPELLCHESS_PROFILE=1 ~/.cargo/bin/cargo run --release -p spellchess-search --example bench -- 6
SPELLCHESS_THREADS=1 SPELLCHESS_PROFILE=1 ~/.cargo/bin/cargo run --release -p spellchess-search --example bench -- 8
```

- [ ] **Step 2a: Record the results**

Confirm node/qnode counts printed match the baseline exactly: depth 6 = `340473` nodes / `2383276` qnodes, depth 8 = `6453264` / `17437140`. If they differ, STOP — this means the precompute changed the search tree, which is a correctness bug, not a perf regression; do not proceed to Task 5 until this is root-caused (re-check Task 2's implementation against the design doc's algorithm, and re-run Task 1's fixtures).

Record the wall-clock times reported by `bench` for depth 6 and depth 8 in the final task (Task 8) commit/summary — no target number is required in advance, per the spec.

---

## Task 5: Scaffold the `cargo-fuzz` crate

**Files:**
- Create: `crates/core/fuzz/Cargo.toml`
- Create: `crates/core/fuzz/fuzz_targets/fuzz_freeze.rs` (stub, filled in Task 6)
- Create: `crates/core/fuzz/fuzz_targets/fuzz_jump.rs` (stub, filled in Task 6)

**Interfaces:**
- Consumes: `~/.cargo/bin/cargo +nightly fuzz init`/`add` (already confirmed working in this environment during brainstorming).
- Produces: a buildable (currently no-op) fuzz crate for Task 6 to fill in.

- [ ] **Step 1: Initialize the fuzz crate**

```bash
cd "/home/alex/Spell Chess/crates/core"
~/.cargo/bin/cargo +nightly fuzz init
```

This creates `crates/core/fuzz/` with a default `fuzz_target_1` target.

- [ ] **Step 2: Add the two real targets and remove the default stub**

```bash
cd "/home/alex/Spell Chess/crates/core"
~/.cargo/bin/cargo +nightly fuzz add fuzz_freeze
~/.cargo/bin/cargo +nightly fuzz add fuzz_jump
rm fuzz/fuzz_targets/fuzz_target_1.rs
```

- [ ] **Step 3: Edit `fuzz/Cargo.toml` to remove the default target's `[[bin]]` entry**

Read `crates/core/fuzz/Cargo.toml` after step 2 and remove the `[[bin]]` block whose `name = "fuzz_target_1"` (added by `cargo fuzz init`), keeping the two blocks for `fuzz_freeze` and `fuzz_jump` that `cargo fuzz add` appended. The file should end up with a `[dependencies]` section (`libfuzzer-sys = "0.4"` and a path dependency on `spellchess-core`) plus exactly two `[[bin]]` sections.

- [ ] **Step 4: Confirm the crate still builds with placeholder target bodies**

```bash
cd "/home/alex/Spell Chess/crates/core"
~/.cargo/bin/cargo +nightly fuzz build
```

Expected: builds successfully (the `cargo fuzz add`-generated stub bodies are no-ops that compile as-is).

- [ ] **Step 5: Commit**

```bash
cd "/home/alex/Spell Chess"
git add crates/core/fuzz/Cargo.toml crates/core/fuzz/fuzz_targets/
git commit -m "$(cat <<'EOF'
chore(core): scaffold the spell_delta cargo-fuzz crate

Two targets, fuzz_freeze and fuzz_jump, still stubs -- filled in next.
EOF
)"
```

---

## Task 6: Write the byte-driven position generator and the two fuzz target bodies

**Files:**
- Modify: `crates/core/fuzz/fuzz_targets/fuzz_freeze.rs`
- Modify: `crates/core/fuzz/fuzz_targets/fuzz_jump.rs`

**Interfaces:**
- Consumes: `spellchess_core::{Position, Board, CastleRights, Piece, Color, PieceKind, Square, SpellField, SpellKind, SpellCast, PieceMove, Delta, captures_enabled_by, legal_moves, spells}` (all `pub`/re-exported from `crates/core/src/lib.rs`).
- Produces: nothing consumed elsewhere — these are terminal fuzz binaries.

Both files are near-identical (duplicated deliberately, matching the existing precedent of `spell_delta.rs`'s own `mod tests` keeping its own copy of `rescan_oracle` alongside `tests/spell_delta_soundness.rs`'s copy, each cross-referencing the others rather than sharing a module) — only the `SpellKind`/target-list line differs.

- [ ] **Step 1: Write `crates/core/fuzz/fuzz_targets/fuzz_freeze.rs`**

```rust
#![no_main]

use libfuzzer_sys::fuzz_target;
use spellchess_core::*;

/// Reads pseudo-random choices from the fuzzer's raw bytes, returning 0 once
/// exhausted so short/mutated inputs still decode to *some* (boring) position
/// instead of failing -- libFuzzer's own coverage-guided mutation is what
/// grows more interesting inputs from there, not this reader.
struct ByteReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> ByteReader<'a> {
    fn new(data: &'a [u8]) -> ByteReader<'a> {
        ByteReader { data, pos: 0 }
    }

    fn next_u8(&mut self) -> u8 {
        let b = self.data.get(self.pos).copied().unwrap_or(0);
        self.pos += 1;
        b
    }

    fn range(&mut self, n: usize) -> usize {
        self.next_u8() as usize % n
    }
}

const KINDS: [PieceKind; 5] =
    [PieceKind::Pawn, PieceKind::Knight, PieceKind::Bishop, PieceKind::Rook, PieceKind::Queen];

fn adjacent(a: Square, b: Square) -> bool {
    a != b && a.file().abs_diff(b.file()) <= 1 && a.rank().abs_diff(b.rank()) <= 1
}

fn take(r: &mut ByteReader, free: &mut Vec<Square>) -> Square {
    let idx = r.range(free.len());
    free.remove(idx)
}

fn place(r: &mut ByteReader, free: &mut Vec<Square>, pos: &mut Position, sq: Square) {
    free.retain(|&s| s != sq);
    let kind = KINDS[r.range(KINDS.len())];
    let color = if r.next_u8() & 1 == 0 { Color::White } else { Color::Black };
    if kind == PieceKind::Pawn && (sq.rank() == 0 || sq.rank() == 7) {
        return;
    }
    pos.board.set(sq, Some(Piece { color, kind }));
}

/// Byte-driven counterpart of `king_tangle_positions` in
/// `crates/core/tests/spell_delta_soundness.rs` -- same construction (kings
/// placed, 1-3 pieces crowding them, 1-4 more anywhere, up to 2 spell fields
/// half-anchored on a king), sourced from fuzzer bytes instead of a
/// splitmix64 seed so libFuzzer's coverage-guided mutation can steer it.
/// Duplicated rather than shared, matching the existing precedent of
/// `spell_delta.rs`'s own `mod tests` keeping its own copy of `rescan_oracle`.
fn decode_position(r: &mut ByteReader) -> Position {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.castle_rights = CastleRights {
        white_kingside: false,
        white_queenside: false,
        black_kingside: false,
        black_queenside: false,
    };
    let mut free: Vec<Square> = (0..64).map(Square).collect();

    let wk = take(r, &mut free);
    let bk = take(r, &mut free);
    pos.board.set(wk, Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(bk, Some(Piece { color: Color::Black, kind: PieceKind::King }));

    for _ in 0..(1 + r.range(3)) {
        let ring: Vec<Square> =
            free.iter().copied().filter(|&s| adjacent(s, wk) || adjacent(s, bk)).collect();
        if ring.is_empty() {
            break;
        }
        let sq = ring[r.range(ring.len())];
        place(r, &mut free, &mut pos, sq);
    }
    for _ in 0..(1 + r.range(4)) {
        if free.is_empty() {
            break;
        }
        let sq = take(r, &mut free);
        place(r, &mut free, &mut pos, sq);
    }

    pos.side_to_move = if r.next_u8() & 1 == 0 { Color::White } else { Color::Black };
    pos.ply = 8;
    let our_king = if pos.side_to_move == Color::White { wk } else { bk };
    for _ in 0..r.range(3) {
        let square = if r.next_u8() & 1 == 0 {
            let near: Vec<Square> =
                (0..64).map(Square).filter(|&s| s == our_king || adjacent(s, our_king)).collect();
            near[r.range(near.len())]
        } else {
            Square((r.next_u8() as usize % 64) as u8)
        };
        let kind = if r.next_u8() & 1 == 0 { SpellKind::Freeze } else { SpellKind::Jump };
        if pos.fields.iter().any(|f| f.kind == kind && f.square == square) {
            continue;
        }
        if kind == SpellKind::Jump && pos.board.get(square).is_none() {
            continue;
        }
        pos.fields.push(SpellField {
            square,
            owner: pos.side_to_move.opposite(),
            kind,
            expires_after_ply: pos.ply,
        });
    }
    pos
}

/// Exactly what `check_kind_over_ordered` in `tests/spell_delta_soundness.rs`
/// checks: whenever the delta returns `Complete`, it must agree with the
/// rescan in set *and* emission order.
fn rescan_oracle(pos: &Position, cast: SpellCast, baseline: &[PieceMove]) -> Vec<PieceMove> {
    let mut next = *pos;
    next.fields.push(SpellField {
        square: cast.square,
        owner: pos.side_to_move,
        kind: cast.kind,
        expires_after_ply: pos.ply + 1,
    });
    legal_moves(&next)
        .into_iter()
        .filter(|mv| {
            let is_cap = pos.board.get(mv.to).is_some() || mv.is_en_passant;
            is_cap && !baseline.contains(mv)
        })
        .collect()
}

fn key(mv: &PieceMove) -> (u8, u8, u8, bool, bool) {
    (mv.from.0, mv.to.0, mv.promotion.map(|p| p as u8).unwrap_or(255), mv.is_en_passant, mv.is_castle)
}

fuzz_target!(|data: &[u8]| {
    let mut r = ByteReader::new(data);
    let pos = decode_position(&mut r);
    let baseline = legal_moves(&pos);
    for square in spells::freeze_targets(&pos, pos.side_to_move) {
        let cast = SpellCast { kind: SpellKind::Freeze, square };
        let mut fast = Vec::new();
        if captures_enabled_by(&pos, cast, &baseline, &mut fast) == Delta::Complete {
            let want = rescan_oracle(&pos, cast, &baseline);
            let mut got_sorted: Vec<_> = fast.iter().map(key).collect();
            let mut want_sorted: Vec<_> = want.iter().map(key).collect();
            got_sorted.sort();
            want_sorted.sort();
            assert_eq!(got_sorted, want_sorted, "SET MISMATCH for {cast:?} on {:?}", pos.board);
            let got_raw: Vec<_> = fast.iter().map(key).collect();
            let want_raw: Vec<_> = want.iter().map(key).collect();
            assert_eq!(got_raw, want_raw, "ORDER MISMATCH for {cast:?} on {:?}", pos.board);
        }
    }
});
```

- [ ] **Step 2: Write `crates/core/fuzz/fuzz_targets/fuzz_jump.rs`**

Identical to `fuzz_freeze.rs` above, except the `fuzz_target!` body loops over `spells::jump_targets` with `SpellKind::Jump`:

```rust
fuzz_target!(|data: &[u8]| {
    let mut r = ByteReader::new(data);
    let pos = decode_position(&mut r);
    let baseline = legal_moves(&pos);
    for square in spells::jump_targets(&pos, pos.side_to_move) {
        let cast = SpellCast { kind: SpellKind::Jump, square };
        let mut fast = Vec::new();
        if captures_enabled_by(&pos, cast, &baseline, &mut fast) == Delta::Complete {
            let want = rescan_oracle(&pos, cast, &baseline);
            let mut got_sorted: Vec<_> = fast.iter().map(key).collect();
            let mut want_sorted: Vec<_> = want.iter().map(key).collect();
            got_sorted.sort();
            want_sorted.sort();
            assert_eq!(got_sorted, want_sorted, "SET MISMATCH for {cast:?} on {:?}", pos.board);
            let got_raw: Vec<_> = fast.iter().map(key).collect();
            let want_raw: Vec<_> = want.iter().map(key).collect();
            assert_eq!(got_raw, want_raw, "ORDER MISMATCH for {cast:?} on {:?}", pos.board);
        }
    }
});
```

(everything above `fuzz_target!` in this file — `ByteReader`, `KINDS`, `adjacent`, `take`, `place`, `decode_position`, `rescan_oracle`, `key` — is the identical copy from `fuzz_freeze.rs`, Step 1.)

- [ ] **Step 3: Build both targets**

```bash
cd "/home/alex/Spell Chess/crates/core"
~/.cargo/bin/cargo +nightly fuzz build
```

Expected: both `fuzz_freeze` and `fuzz_jump` compile without warnings-as-errors issues (fix any unused-import/dead-code warnings that surface before moving on).

- [ ] **Step 4: Commit**

```bash
cd "/home/alex/Spell Chess"
git add crates/core/fuzz/fuzz_targets/
git commit -m "$(cat <<'EOF'
feat(core): implement the freeze and jump spell_delta fuzz targets

Byte-driven port of king_tangle_positions feeds libFuzzer's
coverage-guided mutation; both targets run the same set-and-order
differential check against rescan_oracle that the hand-written test
suite uses.
EOF
)"
```

---

## Task 7: Gitignore, smoke-run both targets, commit

**Files:**
- Modify: `.gitignore`

**Interfaces:** none.

- [ ] **Step 1: Add fuzz output directories to `.gitignore`**

Append to `/home/alex/Spell Chess/.gitignore`:

```
# cargo-fuzz corpus/crash output for crates/core/fuzz -- its target/ dir is
# already covered by the blanket `target/` rule above.
crates/core/fuzz/corpus/
crates/core/fuzz/artifacts/
```

- [ ] **Step 2: Smoke-run each target for a bounded time**

```bash
cd "/home/alex/Spell Chess/crates/core"
~/.cargo/bin/cargo +nightly fuzz run fuzz_freeze -- -max_total_time=120
~/.cargo/bin/cargo +nightly fuzz run fuzz_jump -- -max_total_time=120
```

Expected: both run for ~2 minutes and exit cleanly (no crash/assertion). If either finds a crash: read the printed reproducer path, minimize it (`~/.cargo/bin/cargo +nightly fuzz tmin fuzz_freeze <artifact-path>` or the `fuzz_jump` equivalent), and **stop this task** — a real finding here means going back and adding a hand-authored regression fixture to `spell_delta_soundness.rs` per this project's standing policy (see the spec's "Regression policy" section) before continuing. Report the finding rather than silently working around it.

- [ ] **Step 3: Commit the gitignore change**

```bash
git add .gitignore
git commit -m "$(cat <<'EOF'
chore: gitignore cargo-fuzz corpus/artifacts output for crates/core/fuzz
EOF
)"
```

---

## Task 8: Final verification and merge

**Files:** none modified — verification and merge only.

- [ ] **Step 1: Run the full workspace suite one more time**

```bash
~/.cargo/bin/cargo test --workspace --release 2>&1 | tail -60
```

Expected: all pass.

- [ ] **Step 2: Re-confirm the release perf gate and node counts**

```bash
~/.cargo/bin/cargo test -p spellchess-search --release -- --ignored
```

Expected: PASS, same as Task 4.

- [ ] **Step 3: Merge to `main`**

```bash
cd "/home/alex/Spell Chess"
git checkout main
git merge --no-ff perf/jump-exposure-and-fuzz-harness -m "$(cat <<'EOF'
Merge branch 'perf/jump-exposure-and-fuzz-harness'

Replaces jump_captures' per-target attackers_to call with a per-node
jump-exposure precompute (item 14 TODO 1), and adds a cargo-fuzz
harness over both spell deltas as a second, coverage-guided discovery
method alongside hand-built geometries and mutation testing.
EOF
)"
git branch -d perf/jump-exposure-and-fuzz-harness
```

- [ ] **Step 4: Report final numbers**

Summarize for the user: depth 6 and depth 8 wall-clock before/after (from Task 4), confirmation that node counts stayed bit-identical, and whether the fuzz smoke-run (Task 7) found anything.

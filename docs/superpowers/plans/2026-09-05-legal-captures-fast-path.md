# Captures-only legal-move fast path Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the freeze/jump quiescence rescan's `legal_moves(&hypo).filter(is_capture)` with a captures-only `legal_captures(&hypo)` that skips generating the quiet moves that get thrown away, without changing any freeze/jump legality reasoning.

**Architecture:** A new `pseudo_legal_captures` in `movegen.rs` mirrors `pseudo_legal_moves`'s per-piece-kind structure and iteration order but only emits capture destinations. `legal_moves`'s existing 6-filter closure is extracted into a shared `move_survives` predicate driven by a `LegalCtx`, so `legal_moves` and a new `legal_captures` differ only in which pseudo-legal generator feeds the same filter. Two call sites in `legal.rs` swap to the new function; everything else is unchanged.

**Tech Stack:** Rust, `spellchess-core` crate (zero non-dev dependencies), existing hand-built/oracle/differential test suites.

**Spec:** `docs/superpowers/specs/2026-09-05-legal-captures-fast-path-design.md`

## Global Constraints

- `cargo` may not be on `PATH` in some shells — use `~/.cargo/bin/cargo` if plain `cargo` fails.
- No new dependencies — `spellchess-core` has zero non-dev dependencies; this plan needs none.
- Perf measurement must use `cargo build --release --workspace --bins --examples` (not `--examples` alone, which silently skips rebuilding `target/release/spellchess` and stale-binary-compares against fresh examples) and the committed `bench`/`qprof` examples, never the CLI REPL.
- Node/qnode counts must stay bit-identical to the pre-change baseline at depth 6 and depth 8: **340473/2383276** (depth 6), **6453264/17437140** (depth 8). Read via `SPELLCHESS_PROFILE=1` with the `bench` example (single-threaded: `SPELLCHESS_THREADS=1`).
- Baseline wall-clock this session (release, single-threaded, `bench` example, `main` at `6c8fc2a`): depth 6 = 6.845s, depth 8 = 96.256s. No new target number is committed to — report whatever the change actually measures.
- Mutation-test new/moved guards with `--no-fail-fast` (plain `cargo test` stops at the first failing target and mis-attributes kills).
- This is a pure optimization: no observable behavior may change anywhere in the tree outside the two rescan call sites named in the spec.

---

### Task 1: `pseudo_legal_captures` in `movegen.rs`

**Files:**
- Modify: `crates/core/src/movegen.rs`

**Interfaces:**
- Produces: `pub fn pseudo_legal_captures(pos: &Position) -> Vec<PieceMove>` — public, same crate-visibility as `pseudo_legal_moves`. Task 2/3 in `legal.rs` will import and call this.

- [ ] **Step 1: Write the failing tests**

Add to the bottom of `crates/core/src/movegen.rs`'s existing `#[cfg(test)] mod tests` block (after the last existing test, before the closing `}`), and add `use crate::board::Board;` to that module's `use` list (it currently has `use super::*; use crate::position::Position; use crate::types::PieceKind;` — `Color`, `Piece`, `Square` already reach the module via `super::*`):

```rust
    fn captures_from(pos: &Position, sq: Square) -> Vec<Square> {
        let mut v: Vec<Square> = pseudo_legal_captures(pos).into_iter().filter(|m| m.from == sq).map(|m| m.to).collect();
        v.sort();
        v
    }

    #[test]
    fn pawn_captures_diagonally_but_not_by_pushing() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e4").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("d5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("f5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        let mut expected = vec![Square::from_str("d5").unwrap(), Square::from_str("f5").unwrap()];
        expected.sort();
        assert_eq!(captures_from(&pos, Square::from_str("e4").unwrap()), expected);
    }

    #[test]
    fn pawn_en_passant_capture_is_included() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e5").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("d5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.en_passant = Some(Square::from_str("d6").unwrap());
        let caps = pseudo_legal_captures(&pos);
        let found = caps.iter().find(|m| m.from == Square::from_str("e5").unwrap() && m.to == Square::from_str("d6").unwrap());
        assert!(found.is_some_and(|m| m.is_en_passant), "en passant capture must be included and flagged");
    }

    #[test]
    fn pawn_capture_promotion_included_quiet_promotion_excluded() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("b7").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("a8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        let caps = pseudo_legal_captures(&pos);
        let b7 = Square::from_str("b7").unwrap();
        let a8 = Square::from_str("a8").unwrap();
        let b8 = Square::from_str("b8").unwrap();
        assert_eq!(
            caps.iter().filter(|m| m.from == b7 && m.to == a8).count(), 4,
            "all four promotion pieces must appear for the capture on a8",
        );
        assert!(!caps.iter().any(|m| m.from == b7 && m.to == b8), "quiet promotion to b8 (no capture) must not appear");
    }

    #[test]
    fn knight_captures_only_enemy_occupied_squares() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("d4").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Knight }));
        pos.board.set(Square::from_str("c6").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("e6").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        assert_eq!(captures_from(&pos, Square::from_str("d4").unwrap()), vec![Square::from_str("c6").unwrap()]);
    }

    #[test]
    fn king_captures_only_enemy_occupied_squares() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e4").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("e5").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        assert_eq!(captures_from(&pos, Square::from_str("e4").unwrap()), vec![Square::from_str("d5").unwrap()]);
    }

    #[test]
    fn slider_captures_through_a_live_jump_square() {
        use crate::position::{SpellField, SpellKind};
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("d1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("d4").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("d8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        assert!(captures_from(&pos, Square::from_str("d1").unwrap()).is_empty(), "fixture is wrong: d1 must not see d8 before the jump");
        pos.fields.push(SpellField {
            square: Square::from_str("d4").unwrap(),
            owner: Color::White,
            kind: SpellKind::Jump,
            expires_after_ply: pos.ply + 1,
        });
        assert_eq!(captures_from(&pos, Square::from_str("d1").unwrap()), vec![Square::from_str("d8").unwrap()]);
    }

    #[test]
    fn castling_never_appears_as_a_capture() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("h1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        assert!(pseudo_legal_moves(&pos).iter().any(|m| m.is_castle), "fixture is wrong: castling must be pseudo-legal here");
        assert!(!pseudo_legal_captures(&pos).iter().any(|m| m.is_castle));
    }

    #[test]
    fn pseudo_legal_captures_matches_pseudo_legal_moves_filtered_ordered() {
        fn is_cap(pos: &Position, mv: &PieceMove) -> bool {
            pos.board.get(mv.to).is_some() || mv.is_en_passant
        }
        let mut battery = vec![Position::starting()];
        let mut sparse = Position { board: Board::empty(), ..Position::starting() };
        sparse.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        sparse.board.set(Square::from_str("d1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        sparse.board.set(Square::from_str("d4").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        sparse.board.set(Square::from_str("d8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        sparse.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        battery.push(sparse);
        for pos in battery {
            let expected: Vec<PieceMove> = pseudo_legal_moves(&pos).into_iter().filter(|m| is_cap(&pos, m)).collect();
            let actual = pseudo_legal_captures(&pos);
            assert_eq!(actual, expected, "order/set mismatch for side_to_move {:?}", pos.side_to_move);
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail with "not found"**

Run: `~/.cargo/bin/cargo test -p spellchess-core --lib movegen`
Expected: FAIL to compile — `pseudo_legal_captures` not found.

- [ ] **Step 3: Implement `pseudo_legal_captures`**

Add to `crates/core/src/movegen.rs`, directly after `pseudo_legal_moves` (before the `#[cfg(test)]` block):

```rust
fn pawn_capture_moves(pos: &Position, sq: Square, color: Color, out: &mut Vec<PieceMove>) {
    let dir: i8 = if color == Color::White { 1 } else { -1 };
    let promo_rank: u8 = if color == Color::White { 7 } else { 0 };
    let (f, r) = (sq.file() as i8, sq.rank() as i8);
    for df in [-1i8, 1i8] {
        let (cf, cr) = (f + df, r + dir);
        if !in_bounds(cf, cr) {
            continue;
        }
        let dest = Square::new(cf as u8, cr as u8);
        if let Some(p) = pos.board.get(dest) {
            if p.color != color {
                add_pawn_move(out, sq, dest, false, promo_rank);
            }
        } else if pos.en_passant == Some(dest) {
            add_pawn_move(out, sq, dest, true, promo_rank);
        }
    }
}

fn slide_capture_moves(from: Square, enemy_bb: Bitboard, slider_occ: Bitboard, bishop: bool, rook: bool, out: &mut Vec<PieceMove>) {
    let mut attacks = Bitboard::EMPTY;
    if bishop {
        attacks = attacks.union(bishop_attacks(from, slider_occ));
    }
    if rook {
        attacks = attacks.union(rook_attacks(from, slider_occ));
    }
    for to in attacks.intersect(enemy_bb).iter() {
        out.push(PieceMove::quiet(from, to));
    }
}

/// Captures-only sibling of `pseudo_legal_moves`. Mirrors its exact per-square
/// iteration order (`own.minus(frozen)`, ascending) and per-`PieceKind` dispatch, so
/// filtering `pseudo_legal_moves`'s output down to captures yields precisely this
/// function's output, in the same relative order -- see
/// `pseudo_legal_captures_matches_pseudo_legal_moves_filtered_ordered` below. Pushes
/// directly into one shared `out` (no per-piece `Vec` allocation via `pawn_moves`/
/// `slide_dests`, unlike `pseudo_legal_moves`) since the saving this exists for is as
/// much about allocation count as element count. Castling is never a capture, so
/// `castle_moves` is never called here.
pub fn pseudo_legal_captures(pos: &Position) -> Vec<PieceMove> {
    let color = pos.side_to_move;
    let frozen = crate::spells::frozen_bb(pos);
    let jump = crate::spells::jump_bb(pos);
    let occ = pos.board.occupancy();
    let own = pos.board.color_bb(color);
    let enemy_bb = pos.board.color_bb(color.opposite());
    let slider_occ = occ.minus(jump);
    let mut out = Vec::new();
    for sq in own.minus(frozen).iter() {
        let piece = pos.board.get(sq).expect("color bit set");
        match piece.kind {
            PieceKind::Pawn => pawn_capture_moves(pos, sq, color, &mut out),
            PieceKind::Knight => {
                for dest in KNIGHT_ATTACKS[sq.0 as usize].intersect(enemy_bb).iter() {
                    out.push(PieceMove::quiet(sq, dest));
                }
            }
            PieceKind::King => {
                for dest in KING_ATTACKS[sq.0 as usize].intersect(enemy_bb).iter() {
                    out.push(PieceMove::quiet(sq, dest));
                }
            }
            PieceKind::Bishop => slide_capture_moves(sq, enemy_bb, slider_occ, true, false, &mut out),
            PieceKind::Rook => slide_capture_moves(sq, enemy_bb, slider_occ, false, true, &mut out),
            PieceKind::Queen => slide_capture_moves(sq, enemy_bb, slider_occ, true, true, &mut out),
        }
    }
    out
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `~/.cargo/bin/cargo test -p spellchess-core --lib movegen`
Expected: PASS, all 8 new tests plus the existing `movegen::tests` suite.

- [ ] **Step 5: Run the full existing core test suite to check for regressions**

Run: `~/.cargo/bin/cargo test -p spellchess-core`
Expected: PASS, no regressions (this task adds a new function and tests only; nothing existing calls it yet).

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/movegen.rs
git commit -m "$(cat <<'EOF'
feat(core): add pseudo_legal_captures, a captures-only pseudo-legal generator

Mirrors pseudo_legal_moves' structure and iteration order but only emits
capture destinations, with no per-piece Vec allocation. Not wired into
anything yet -- crates/core/src/legal.rs will consume it next.
EOF
)"
```

---

### Task 2: Extract `LegalCtx`/`move_survives` from `legal_moves` (pure refactor)

**Files:**
- Modify: `crates/core/src/legal.rs:68-155` (the `LEGAL_MOVE_FILTERS` const and `legal_moves`)
- Modify: `crates/core/src/spell_delta.rs` (two comment updates, no logic change)

**Interfaces:**
- Consumes: nothing new from Task 1 yet (this task only refactors `legal.rs` internals).
- Produces: `pub(crate) struct LegalCtx { mover: Color, enemy: Color, king_sq: Square, checker_count: u32, checkers: Bitboard, pins: PinMap, jump: Bitboard }`, `fn legal_ctx(pos: &Position, mover: Color) -> Option<LegalCtx>`, `fn move_survives(pos: &Position, mv: &PieceMove, ctx: &LegalCtx) -> bool`. `legal_moves`'s public signature and behavior are unchanged. Task 3 will call `legal_ctx` and `move_survives` to build `legal_captures`.

This task is a pure refactor with no new behavior — there is no new test to write first. The gate is that every existing test still passes identically afterward.

- [ ] **Step 1: Run the full test suite before changing anything, to record the passing baseline**

Run: `~/.cargo/bin/cargo test --workspace`
Expected: PASS (record this as the "before" state — this task's contract is that step 4 below reproduces the exact same PASS).

- [ ] **Step 2: Replace the `LEGAL_MOVE_FILTERS` const and `legal_moves` body**

In `crates/core/src/legal.rs`, replace lines 68-155 (from `/// How many distinct filters...` through the closing `}` of `legal_moves`) with:

```rust
/// How many distinct filters `move_survives` applies to a pseudo-legal move. Pinned
/// by a static assertion in `spell_delta`, which enumerates all of them; see the
/// mapping table above the filter chain below.
pub(crate) const LEGAL_MOVE_FILTERS: usize = 6;

pub(crate) struct LegalCtx {
    mover: Color,
    enemy: Color,
    king_sq: Square,
    checker_count: u32,
    checkers: Bitboard,
    pins: PinMap,
    jump: Bitboard,
}

fn legal_ctx(pos: &Position, mover: Color) -> Option<LegalCtx> {
    let enemy = mover.opposite();
    let king_sq = pos.board.king_square(mover)?;
    let frozen = crate::spells::frozen_bb(pos);
    let jump = crate::spells::jump_bb(pos);
    let occ = pos.board.occupancy();
    let slider_occ = occ.minus(jump);
    let checkers = crate::attacks::attackers_to(pos, king_sq, enemy, frozen, slider_occ);
    let checker_count = checkers.count();
    let pins = pins_of(pos, king_sq, mover, frozen, jump);
    Some(LegalCtx { mover, enemy, king_sq, checker_count, checkers, pins, jump })
}

// FILTER LIST -- keep in step with `spell_delta::freeze_captures`, whose entire
// correctness argument is an enumeration of these. Freeze never changes
// reachability, so every capture a freeze newly makes legal is a move that was
// already pseudo-legal and that one of the six filters below used to reject.
// `freeze_captures` either models that filter or declines to `NeedsRescan`:
//
//   1. enemy-king capture, `after_count <= before_count.max(1)`  -> declined
//      (freeze_captures' "mechanism 4" guard)
//   2. en passant clone-and-rescan                               -> declined
//      (its `pos.en_passant.is_some()` guard)
//   3. king move, `king_dest_safe`                               -> MODELLED
//      (its mechanism 3)
//   4. `checker_count >= 2`                                      -> declined
//      (its `ctx.checkers` non-empty guard)
//   5. pin ray, plus the live-jump-square clone-and-rescan arm   -> MODELLED
//      (its mechanism 2) / declined (its `jump_enemy` guard)
//   6. `checker_count == 1`, `evasion_allows`                    -> declined
//      (the same `ctx.checkers` guard)
//
// Changing this list means `LEGAL_MOVE_FILTERS` above no longer matches and
// `spell_delta`'s static assertion fails the build. That is a tripwire, not a
// proof -- it only fires if you update the count, and the differential batteries
// only catch a missed filter when they happen to generate the geometry. Read
// `freeze_captures`' doc comment before touching anything here.
fn move_survives(pos: &Position, mv: &PieceMove, ctx: &LegalCtx) -> bool {
    let dest_piece = pos.board.get(mv.to);
    if dest_piece.is_some_and(|p| p.kind == PieceKind::King && p.color == ctx.enemy) {
        let after = apply_move_only(pos, mv);
        let after_sq = after.board.king_square(ctx.mover).expect("own king remains");
        let before_count = crate::attacks::attacker_count(pos, ctx.king_sq, ctx.enemy);
        let after_count = crate::attacks::attacker_count(&after, after_sq, ctx.enemy);
        return after_count <= before_count.max(1);
    }
    if mv.is_en_passant {
        let after = apply_move_only(pos, mv);
        return !crate::attacks::is_square_attacked(&after, after.board.king_square(ctx.mover).unwrap(), ctx.enemy);
    }
    let mover_piece = pos.board.get(mv.from).unwrap();
    if mover_piece.kind == PieceKind::King {
        return king_dest_safe(pos, ctx.king_sq, mv.to, ctx.enemy);
    }
    if ctx.checker_count >= 2 {
        return false;
    }
    if let Some(ray) = pin_ray(&ctx.pins, mv.from) {
        if !ray.contains(mv.to) {
            return false;
        }
        // Landing on a jump square stays transparent. Capturing a jumped
        // *pinner* can still be legal; capturing a jumped *between* piece
        // is not. Clone-and-rescan distinguishes those.
        if ctx.jump.contains(mv.to) {
            let after = apply_move_only(pos, mv);
            return after
                .board
                .king_square(ctx.mover)
                .is_none_or(|k| !crate::attacks::is_square_attacked(&after, k, ctx.enemy));
        }
    }
    if ctx.checker_count == 1 {
        let checker = ctx.checkers.iter().next().unwrap();
        return evasion_allows(pos, ctx.king_sq, checker, mv.to, ctx.jump);
    }
    true
}

pub fn legal_moves(pos: &Position) -> Vec<PieceMove> {
    let mover = pos.side_to_move;
    let Some(ctx) = legal_ctx(pos, mover) else { return Vec::new() };
    pseudo_legal_moves(pos).into_iter().filter(|mv| move_survives(pos, mv, &ctx)).collect()
}
```

- [ ] **Step 3: Update the two `spell_delta.rs` comments that name `legal_moves`' filter list**

In `crates/core/src/spell_delta.rs`, the doc comment above `freeze_captures` currently reads (around line 433-450):

```rust
/// Freeze never changes reachability, only control (rules/30-freeze.md: frozen
/// pieces "exert no control" but "still block sliding pieces"). Occupancy is
/// untouched, so `pseudo_legal_moves` can only shrink; every newly *legal* move was
/// already pseudo-legal and was rejected by one of `legal_moves`' filters. Freeze
/// can therefore only flip one of these:
```
and further down:
```rust
/// That list is the spec. The `const _` assertion just below pins `legal_moves`'
/// filter count so that adding or removing a filter there breaks this build -- a
/// tripwire, not a proof, since it depends on whoever edits `legal_moves` updating
/// the count the comment there tells them to update.
const _: () = assert!(
    crate::legal::LEGAL_MOVE_FILTERS == 6,
    "legal_moves' filter list changed -- revisit freeze_captures' four-mechanism enumeration",
);
```

Change `` already pseudo-legal and was rejected by one of `legal_moves`' filters. `` to `` already pseudo-legal and was rejected by one of `move_survives`' filters. ``, and change the second block to:

```rust
/// That list is the spec. The `const _` assertion just below pins `move_survives`'
/// filter count so that adding or removing a filter there breaks this build -- a
/// tripwire, not a proof, since it depends on whoever edits `move_survives` updating
/// the count the comment there tells them to update.
const _: () = assert!(
    crate::legal::LEGAL_MOVE_FILTERS == 6,
    "move_survives' filter list changed -- revisit freeze_captures' four-mechanism enumeration",
);
```

- [ ] **Step 4: Run the full test suite and confirm it reproduces the pre-refactor PASS**

Run: `~/.cargo/bin/cargo test --workspace`
Expected: PASS — identical outcome to Step 1. This exercises every hand-built vector in `legal.rs`'s own `mod tests` (`vector_1` through `vector_19`, `king_cannot_move_into_check`), the full `oracle_vectors.rs` fixture set, and every differential battery in `spell_delta_soundness.rs` (all of which call `legal_moves` via `rescan_oracle`/`with_field`) — together these are exactly the "hand-built fixtures, oracle fixtures, and differential sweep" the spec's refactor-safety requirement names. Any behavior change in the extraction would very likely break at least one of them.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/legal.rs crates/core/src/spell_delta.rs
git commit -m "$(cat <<'EOF'
refactor(core): extract move_survives/LegalCtx from legal_moves

Pure refactor, no behavior change -- legal_moves' six-filter closure and
its context (king square, checkers, pins, jump) become a standalone
predicate and struct so a captures-only counterpart can reuse them without
duplicating the filter logic. Verified by the full existing test suite
reproducing an identical pass/fail outcome before and after.
EOF
)"
```

---

### Task 3: `legal_captures` plus internal equivalence and edge-case tests

**Files:**
- Modify: `crates/core/src/legal.rs` (add `legal_captures`, import `pseudo_legal_captures`, add tests to its existing `mod tests`)
- Modify: `crates/core/src/lib.rs:33` (export `legal_captures`)

**Interfaces:**
- Consumes: `pseudo_legal_captures` (Task 1), `legal_ctx`/`move_survives` (Task 2).
- Produces: `pub fn legal_captures(pos: &Position) -> Vec<PieceMove>`, exported from `spellchess_core`. Task 5 will call this at the two rescan call sites.

- [ ] **Step 1: Write the failing tests**

Add to `crates/core/src/legal.rs`'s existing `#[cfg(test)] mod tests` block (after the last existing test, `vector_15_castling_under_freeze`, before the closing `}`):

```rust
    #[test]
    fn legal_captures_matches_legal_moves_filtered_to_captures_ordered() {
        fn splitmix64(state: &mut u64) -> u64 {
            *state = state.wrapping_add(0x9E3779B97F4A7C15);
            let mut z = *state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
            z ^ (z >> 31)
        }
        fn random_legal_walk(seed: u64, plies: u32) -> Vec<Position> {
            let mut state = seed;
            let mut pos = Position::starting();
            let mut out = vec![pos];
            for _ in 0..plies {
                let turns = crate::legal::generate_turns(&pos);
                if turns.is_empty() {
                    break;
                }
                let pick = (splitmix64(&mut state) as usize) % turns.len();
                pos = crate::legal::apply_turn(&pos, &turns[pick]);
                if pos.board.king_square(Color::White).is_none() || pos.board.king_square(Color::Black).is_none() {
                    break;
                }
                out.push(pos);
            }
            out
        }

        let mut battery = vec![Position::starting()];
        for seed in [1u64, 7, 42, 1234] {
            battery.extend(random_legal_walk(seed, 12));
        }

        for pos in battery {
            let expected: Vec<PieceMove> = legal_moves(&pos).into_iter().filter(|mv| is_capture(&pos, mv)).collect();
            let actual = legal_captures(&pos);
            assert_eq!(actual, expected, "legal_captures diverged from legal_moves-filtered-to-captures");
        }
    }

    #[test]
    fn legal_captures_only_king_survives_in_double_check() {
        // Bd7 and Re1 both attack e8: double check. Kxd7 is undefended and safe, so
        // it must survive; Black's knight also has a pseudo-legal Nxg6, which must
        // be filtered out since only king moves are legal under double check.
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d7").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Bishop }));
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("g6").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("h8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Knight }));
        pos.side_to_move = Color::Black;
        assert!(
            crate::attacks::attacker_count(&pos, Square::from_str("e8").unwrap(), Color::White) >= 2,
            "fixture is wrong: this must be a double check",
        );
        let caps = legal_captures(&pos);
        let kxd7 = PieceMove::quiet(Square::from_str("e8").unwrap(), Square::from_str("d7").unwrap());
        let nxg6 = PieceMove::quiet(Square::from_str("h8").unwrap(), Square::from_str("g6").unwrap());
        assert!(caps.contains(&kxd7), "Kxd7 must survive: it resolves both checks and is undefended");
        assert!(!caps.contains(&nxg6), "Nxg6 must be filtered out under double check");
    }

    #[test]
    fn legal_captures_includes_enemy_king_capture_via_jump() {
        // Same geometry as vector_9_king_capture_via_jump.
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Bishop }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("h2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("b4").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Bishop }));
        pos.board.set(Square::from_str("a8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        pos.side_to_move = Color::Black;
        pos.fields.push(crate::position::SpellField {
            square: Square::from_str("d2").unwrap(), owner: Color::Black,
            kind: crate::position::SpellKind::Jump, expires_after_ply: pos.ply + 1,
        });
        let capture = PieceMove::quiet(Square::from_str("b4").unwrap(), Square::from_str("e1").unwrap());
        assert!(legal_captures(&pos).contains(&capture));
    }

    #[test]
    fn legal_captures_includes_slider_capture_through_jump_square() {
        // Same geometry as vector_10_jump_field_serves_both_players.
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("d4").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        pos.fields.push(crate::position::SpellField {
            square: Square::from_str("d4").unwrap(), owner: Color::White,
            kind: crate::position::SpellKind::Jump, expires_after_ply: pos.ply + 1,
        });
        let capture = PieceMove::quiet(Square::from_str("d1").unwrap(), Square::from_str("d8").unwrap());
        assert!(legal_captures(&pos).contains(&capture));
    }

    #[test]
    fn legal_captures_declines_pinned_piece_capturing_onto_live_jump_square() {
        // Same geometry as spell_delta.rs's a_pinned_slider_capturing_onto_a_live_jump_square_declines:
        // after jump@e3, Re2 is pseudo-legally able to take Ne4, but Ne4 sits on a
        // still-transparent square so Re5 sees straight through to Ke1 -- Rxe4 must
        // not be legal.
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("e2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e3").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("e4").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Knight }));
        pos.board.set(Square::from_str("e5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("a8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.fields.push(crate::position::SpellField {
            square: Square::from_str("e4").unwrap(), owner: Color::Black,
            kind: crate::position::SpellKind::Jump, expires_after_ply: pos.ply + 1,
        });
        let cast = SpellCast { kind: crate::position::SpellKind::Jump, square: Square::from_str("e3").unwrap() };
        let hypothetical = position_with_field(&pos, cast);
        let rxe4 = PieceMove::quiet(Square::from_str("e2").unwrap(), Square::from_str("e4").unwrap());
        assert!(!legal_captures(&hypothetical).contains(&rxe4), "Rxe4 must decline: it lands on a still-transparent square");
    }

    #[test]
    fn legal_captures_empty_when_mover_has_no_king() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.side_to_move = Color::White;
        assert!(legal_moves(&pos).is_empty());
        assert!(legal_captures(&pos).is_empty());
    }
```

- [ ] **Step 2: Run tests to verify they fail with "not found"**

Run: `~/.cargo/bin/cargo test -p spellchess-core --lib legal`
Expected: FAIL to compile — `legal_captures` not found.

- [ ] **Step 3: Implement `legal_captures`**

First, update the `use` line near the top of `crates/core/src/legal.rs` from:
```rust
use crate::movegen::{pseudo_legal_moves, PieceMove};
```
to:
```rust
use crate::movegen::{pseudo_legal_captures, pseudo_legal_moves, PieceMove};
```

Then add, directly after `pub fn legal_moves`:

```rust
pub fn legal_captures(pos: &Position) -> Vec<PieceMove> {
    let mover = pos.side_to_move;
    let Some(ctx) = legal_ctx(pos, mover) else { return Vec::new() };
    pseudo_legal_captures(pos).into_iter().filter(|mv| move_survives(pos, mv, &ctx)).collect()
}
```

- [ ] **Step 4: Export `legal_captures` from the crate root**

In `crates/core/src/lib.rs`, change line 33 from:
```rust
pub use legal::{Turn, SpellCast, generate_turns, generate_search_turns, generate_search_spell_turns, generate_quiescence_turns, generate_quiescence_turns_from, generate_quiescence_recapture_turns, legal_moves, apply_move_only, apply_turn};
```
to:
```rust
pub use legal::{Turn, SpellCast, generate_turns, generate_search_turns, generate_search_spell_turns, generate_quiescence_turns, generate_quiescence_turns_from, generate_quiescence_recapture_turns, legal_moves, legal_captures, apply_move_only, apply_turn};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `~/.cargo/bin/cargo test -p spellchess-core --lib legal`
Expected: PASS, all 6 new tests plus the existing `legal::tests` suite (`vector_1` through `vector_19`, etc.).

- [ ] **Step 6: Run the full workspace test suite**

Run: `~/.cargo/bin/cargo test --workspace`
Expected: PASS — `legal_captures` is not yet called from any production call site, so nothing else should be affected.

- [ ] **Step 7: Commit**

```bash
git add crates/core/src/legal.rs crates/core/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(core): add public legal_captures, backed by pseudo_legal_captures

Reuses move_survives unchanged -- same filter, same LegalCtx, different
pseudo-legal source. Verified order-and-set equivalent to
legal_moves().filter(is_capture) over a random-walk battery plus explicit
double-check, enemy-king-capture, jump-square-slider-capture,
pinned-onto-jump-square, and king-less-side fixtures. Not yet wired into
any call site.
EOF
)"
```

---

### Task 4: External fixture-driven sweep with independently-known expected outputs

**Files:**
- Create: `crates/core/tests/legal_captures.rs`

**Interfaces:**
- Consumes: `spellchess_core::{legal_captures, legal_moves, Position, ...}` (public API only — this is an integration test, a separate compilation unit from `spellchess-core`).

This is the layer the spec calls for as distinct from Task 3's internal property test: hand-written expected outputs, not re-derivation of the equivalence property.

- [ ] **Step 1: Write the test file**

Create `crates/core/tests/legal_captures.rs`:

```rust
//! `legal_captures` checked against independently hand-written expected outputs, as
//! a check distinct from `legal.rs`'s own internal order-equivalence property test
//! (see `legal_captures_matches_legal_moves_filtered_to_captures_ordered` there) --
//! this file asserts literal expected move lists, the same way `legal.rs`'s own
//! `vector_*` tests do for `legal_moves`.

use spellchess_core::*;

fn sq(s: &str) -> Square {
    Square::from_str(s).unwrap()
}

#[test]
fn king_capture_via_jump_is_the_only_capture_from_b4() {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(sq("e1"), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(sq("d2"), Some(Piece { color: Color::White, kind: PieceKind::Bishop }));
    pos.board.set(sq("a1"), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(sq("h2"), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(sq("e8"), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(sq("b4"), Some(Piece { color: Color::Black, kind: PieceKind::Bishop }));
    pos.board.set(sq("a8"), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
    pos.side_to_move = Color::Black;
    pos.fields.push(SpellField {
        square: sq("d2"), owner: Color::Black, kind: SpellKind::Jump, expires_after_ply: pos.ply + 1,
    });
    let caps = legal_captures(&pos);
    let from_b4: Vec<Square> = caps.iter().filter(|m| m.from == sq("b4")).map(|m| m.to).collect();
    assert_eq!(from_b4, vec![sq("e1")], "b4's bishop must be able to capture the king on e1 through the jump, and nothing else");
}

#[test]
fn frozen_piece_is_still_capturable_and_the_only_capture_available() {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(sq("e1"), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(sq("d1"), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(sq("e8"), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(sq("d5"), Some(Piece { color: Color::Black, kind: PieceKind::Knight }));
    pos.fields.push(SpellField {
        square: sq("d5"), owner: Color::Black, kind: SpellKind::Freeze, expires_after_ply: pos.ply + 1,
    });
    let caps = legal_captures(&pos);
    let from_d1: Vec<Square> = caps.iter().filter(|m| m.from == sq("d1")).map(|m| m.to).collect();
    assert_eq!(from_d1, vec![sq("d5")], "the frozen knight on d5 must still be capturable, and it's the rook's only capture");
}

#[test]
fn jump_opens_a_rook_battery_capture_for_both_sides() {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(sq("e1"), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(sq("d1"), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(sq("d4"), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
    pos.board.set(sq("e8"), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(sq("d8"), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
    pos.fields.push(SpellField {
        square: sq("d4"), owner: Color::White, kind: SpellKind::Jump, expires_after_ply: pos.ply + 1,
    });
    assert_eq!(
        legal_captures(&pos).iter().filter(|m| m.from == sq("d1")).map(|m| m.to).collect::<Vec<_>>(),
        vec![sq("d8")],
    );
    let mut black_pos = pos;
    black_pos.side_to_move = Color::Black;
    assert_eq!(
        legal_captures(&black_pos).iter().filter(|m| m.from == sq("d8")).map(|m| m.to).collect::<Vec<_>>(),
        vec![sq("d1")],
    );
}

#[test]
fn no_legal_captures_in_the_starting_position() {
    assert!(legal_captures(&Position::starting()).is_empty());
}

#[test]
fn en_passant_is_the_only_capture_available() {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(sq("e1"), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(sq("e5"), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
    pos.board.set(sq("d5"), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
    pos.board.set(sq("e8"), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.en_passant = Some(sq("d6"));
    let caps = legal_captures(&pos);
    assert_eq!(caps.len(), 1);
    assert!(caps[0].is_en_passant && caps[0].from == sq("e5") && caps[0].to == sq("d6"));
}
```

- [ ] **Step 2: Run the new test file to verify it fails without the implementation**

This should already pass since Tasks 1-3 implemented `legal_captures` — run it now purely to confirm the fixtures are correct against the real implementation:

Run: `~/.cargo/bin/cargo test -p spellchess-core --test legal_captures`
Expected: PASS. If any fixture fails, the fixture's hand-derived expectation is wrong (re-derive by hand, do not adjust `legal_captures` to match — cross-check against `legal_moves(&pos).into_iter().filter(|mv| pos.board.get(mv.to).is_some() || mv.is_en_passant)` interactively if unsure).

- [ ] **Step 3: Run the full workspace suite**

Run: `~/.cargo/bin/cargo test --workspace`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/core/tests/legal_captures.rs
git commit -m "$(cat <<'EOF'
test(core): add hand-written expected-output fixtures for legal_captures

Distinct from legal.rs's internal order-equivalence property test: these
assert literal expected capture lists (same style as legal.rs's own
vector_* tests for legal_moves), reusing the king-capture-via-jump,
frozen-piece-still-capturable, and jump-opens-a-battery geometries already
validated elsewhere in the suite.
EOF
)"
```

---

### Task 5: Swap the two rescan call sites to `legal_captures`

**Files:**
- Modify: `crates/core/src/legal.rs:658-666` (`JUMP_RESCAN` arm) and `crates/core/src/legal.rs:698-706` (`FREEZE_RESCAN` arm), both inside `generate_quiescence_inner`

**Interfaces:**
- Consumes: `legal_captures` (Task 3).
- Produces: no new interface — this task only changes what `generate_quiescence_from`'s two rescan arms compute internally. Its output (the `Turn`s pushed into `turns`) must be unchanged.

- [ ] **Step 1: Make the call-site change**

In `crates/core/src/legal.rs`, inside `generate_quiescence_inner`, replace the `JUMP_RESCAN` arm:

```rust
                    crate::spell_delta::Delta::NeedsRescan => {
                        crate::qtime!(JUMP_RESCAN, {
                            for mv in legal_moves(&position_with_field(pos, cast)) {
                                if is_capture(pos, &mv) && !in_baseline(baseline, mv) {
                                    turns.push(Turn { spell: Some(cast), mv });
                                }
                            }
                        });
                    }
```
with:
```rust
                    crate::spell_delta::Delta::NeedsRescan => {
                        crate::qtime!(JUMP_RESCAN, {
                            // `legal_captures` decides captures against `hypo`, not `pos` --
                            // sound only because `position_with_field` touches `fields` alone,
                            // so `board` and `en_passant` (and therefore what counts as a
                            // capture) are identical on both positions.
                            for mv in legal_captures(&position_with_field(pos, cast)) {
                                if !in_baseline(baseline, mv) {
                                    turns.push(Turn { spell: Some(cast), mv });
                                }
                            }
                        });
                    }
```

and the `FREEZE_RESCAN` arm:
```rust
                        crate::spell_delta::Delta::NeedsRescan => {
                            crate::qtime!(FREEZE_RESCAN, {
                                for mv in legal_moves(&position_with_field(pos, cast)) {
                                    if is_capture(pos, &mv) && !in_baseline(baseline, mv) {
                                        turns.push(Turn { spell: Some(cast), mv });
                                    }
                                }
                            });
                        }
```
with:
```rust
                        crate::spell_delta::Delta::NeedsRescan => {
                            crate::qtime!(FREEZE_RESCAN, {
                                // See the identical comment on the JUMP_RESCAN arm above:
                                // position_with_field only touches `fields`, so testing
                                // captures against `hypo` here is equivalent to testing them
                                // against `pos`.
                                for mv in legal_captures(&position_with_field(pos, cast)) {
                                    if !in_baseline(baseline, mv) {
                                        turns.push(Turn { spell: Some(cast), mv });
                                    }
                                }
                            });
                        }
```

- [ ] **Step 2: Add a regression test that actually exercises the changed `FREEZE_RESCAN` arm**

Important: none of the existing differential batteries in `spell_delta_soundness.rs` exercise this change. Their own header comment says so explicitly — they only check agreement when `captures_enabled_by` returns `Complete`; "`NeedsRescan` falls through to that same rescan at every call site, so only `Complete` needs checking" assumed the rescan itself (previously `legal_moves`-based) was correct by construction. This task changes what the rescan computes, so it needs its own direct fixture, not just reliance on existing coverage.

Add to `crates/core/src/legal.rs`'s `mod tests`, after the tests added in Task 3:

```rust
    #[test]
    fn quiescence_offers_a_capture_that_only_exists_after_freeze_dispels_check() {
        // Black's king is in contact check from White's rook on d8. Bxg6 is not
        // legal yet -- it doesn't address the check. Freezing d8 (the checker)
        // dispels the check (a frozen piece exerts no control at all), after which
        // Bxg6 is legal. This is exactly the 81%-of-cases FREEZE_RESCAN shape
        // measured this session: `captures_enabled_by` always declines to
        // NeedsRescan whenever the mover is already in check, so this fixture
        // exercises the arm Task 5 changed, not the FREEZE_DELTA fast path.
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d8").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("g6").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("f7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Bishop }));
        pos.side_to_move = Color::Black;

        let baseline = legal_moves(&pos);
        let bxg6 = PieceMove::quiet(Square::from_str("f7").unwrap(), Square::from_str("g6").unwrap());
        assert!(!baseline.contains(&bxg6), "fixture is wrong: Bxg6 must not address the check and so must start illegal");

        let cast = SpellCast { kind: crate::position::SpellKind::Freeze, square: Square::from_str("d8").unwrap() };
        let turns = generate_quiescence_turns_from(&pos, &baseline);
        assert!(
            turns.iter().any(|t| t.spell == Some(cast) && t.mv == bxg6),
            "freeze@d8 + Bxg6 must appear: freezing the checker dispels check and newly legalizes the bishop capture",
        );
    }
```

- [ ] **Step 3: Run the full workspace test suite**

Run: `~/.cargo/bin/cargo test --workspace`
Expected: PASS, including `quiescence_turns_include_freeze_that_stops_a_recapture`, `cheap_recapture_turns_still_offer_freeze_the_recapturer`, `cheap_recapture_turns_omit_a_jump_enabled_new_capture`, `jump_does_not_pair_king_walks_the_jump_opens_to_check`, and every `spell_delta_soundness.rs` differential battery (`freeze_delta_matches_the_rescan_oracle_on_*`, `jump_delta_matches_the_rescan_oracle_on_*`) — these call `generate_quiescence_from` end-to-end and are the real regression backstop for this call-site change.

- [ ] **Step 4: Run the release perf-bounds suite**

Run: `~/.cargo/bin/cargo test -p spellchess-search --release -- --ignored --test-threads=1`
Expected: PASS (`depth_budget_stays_bounded_on_a_realistic_board`, wall-clock bounds only — this is a smoke check that nothing got catastrophically slower, not the node-count gate; Task 6 does the node-count check).

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/legal.rs
git commit -m "$(cat <<'EOF'
perf(core): use legal_captures at the freeze/jump quiescence rescan sites

FREEZE_RESCAN and JUMP_RESCAN in generate_quiescence_inner previously
called legal_moves(&hypo) -- generating every pseudo-legal quiet move via
pseudo_legal_moves -- then filtered to captures with is_capture. Neither
rescan arm ever wanted the quiet moves. legal_captures skips generating
them. No legality reasoning changes: legal_captures reuses move_survives
unchanged, verified equivalent to legal_moves().filter(is_capture) in the
previous two commits.
EOF
)"
```

---

### Task 6: Measure, verify node-count invariance, and report

**Files:** none (measurement only; may produce a follow-up commit if node counts diverge and step 3 finds a bug)

**Interfaces:** none — this task consumes the finished code from Tasks 1-5 and produces a written result for the spec's Measurement section, not new code.

- [ ] **Step 1: Rebuild the release binaries and examples**

Run: `~/.cargo/bin/cargo build --release --workspace --bins --examples`
Expected: builds cleanly (note: `--bins --examples`, not `--examples` alone, per Global Constraints).

- [ ] **Step 2: Capture node/qnode counts at depth 6 and depth 8, single-threaded, and compare to baseline**

Run:
```bash
SPELLCHESS_THREADS=1 SPELLCHESS_PROFILE=1 ~/.cargo/bin/cargo run --release -p spellchess-search --example bench -- 6
SPELLCHESS_THREADS=1 SPELLCHESS_PROFILE=1 ~/.cargo/bin/cargo run --release -p spellchess-search --example bench -- 8
```
Expected: the `profile ...: nodes=... qnodes=...` line printed to stderr shows **nodes=340473 qnodes=2383276** at depth 6 and **nodes=6453264 qnodes=17437140** at depth 8 — bit-identical to the pre-change baseline (Global Constraints). If either count differs, STOP: this means `legal_captures` is not actually equivalent to `legal_moves(&hypo).filter(is_capture)` in some case the test suite didn't cover. Do not proceed to Step 3 — go back to Task 3/4 and find the missing fixture (likely via `git bisect` against the differential batteries, or by comparing `generate_quiescence_turns_from` output turn-by-turn between this commit and the parent commit on the same position).

- [ ] **Step 3: Measure wall-clock**

Run:
```bash
SPELLCHESS_THREADS=1 ~/.cargo/bin/cargo run --release -p spellchess-search --example bench -- 6
SPELLCHESS_THREADS=1 ~/.cargo/bin/cargo run --release -p spellchess-search --example bench -- 8
```
Record the two `depth N on 1 thread(s): <time> -> Some(<score>)` lines. Compare against this session's baseline (depth 6 = 6.845s, depth 8 = 96.256s). No target is required to pass — report whatever the actual numbers are.

- [ ] **Step 4: Run `qprof` to see the new cost breakdown**

Run: `~/.cargo/bin/cargo run --release -p spellchess-search --example qprof --features spellchess-core/qprofile -- 8`
Record the `FREEZE_RESCAN` and `JUMP_RESCAN` rows' `total ms` and `% total` — compare against this session's pre-change depth-8 numbers (`FREEZE_RESCAN` 6219.4ms/7.6%, `JUMP_RESCAN` 536.7ms/0.7%) to see how much of that specific cost this change removed.

- [ ] **Step 5: Run the full workspace test suite one final time**

Run: `~/.cargo/bin/cargo test --workspace`
Expected: PASS.

- [ ] **Step 6: Write up the result**

No code change — report to the user (and, if this session is the one merging, note for a future `spell-chess-engine-followups` memory update) the four numbers from Steps 2-4: node/qnode counts (must match baseline exactly), wall-clock at depth 6/8, and the `FREEZE_RESCAN`/`JUMP_RESCAN` cost reduction. This closes out the spec's Measurement section.

---

## Self-Review Notes

- **Spec coverage:** `pseudo_legal_captures` (Task 1) → Design section 1. Shared filter extraction (Task 2) → Design section 2, including the two `spell_delta.rs` comment updates the spec calls out by content. `legal_captures` + internal equivalence test (Task 3) → Design section 2 + Testing points 1-2's internal half. External fixture sweep (Task 4) → Testing point 2's external half. Call-site swap + `position_with_field` justification comment (Task 5) → Design section 3. Node-count/wall-clock measurement (Task 6) → Measurement section. All explicit fixtures the spec names (en passant, capture-promotion vs quiet-promotion, pinned-onto-jump-square, double-check, enemy-king capture, jump-square slider capture, king-less side) are covered across Tasks 1 and 3.
- **Placeholder scan:** no TBD/TODO; every step has literal code or an exact command.
- **Type consistency:** `LegalCtx`, `legal_ctx`, `move_survives`, `pseudo_legal_captures`, and `legal_captures` use identical signatures everywhere they're referenced across Tasks 2-5.

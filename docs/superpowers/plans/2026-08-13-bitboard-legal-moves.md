# Bitboard Rewrite and Checkers/Pins `legal_moves` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace mailbox attack/movegen with bitboards and rewrite `legal_moves` to filter against precomputed checkers and pins, so release-build `go --depth 3` on the starting position finishes in under 5 seconds.

**Architecture:** Hybrid `Board` (mailbox + color/kind bitboards + cached kings) and `Copy` `Position`. Slider occupancy is `occupancy & !jump`. Frozen pieces occupy but do not move or attack. Production `legal_moves` uses checkers/pins; clone-and-rescan survives only as a test-only reference and on the rare king-capture / en-passant paths.

**Tech Stack:** Rust, Cargo workspace (`spellchess-core`, `spellchess-search`, `spellchess-cli`), `cargo test` / `cargo test --release`. No new crates.

**Spec:** `docs/superpowers/specs/2026-08-13-bitboard-legal-moves-design.md`

## Global Constraints

- `crates/core` stays at zero non-dev dependencies (`serde`/`serde_json` remain `[dev-dependencies]` only). This plan adds none.
- Every commit message MUST use [Conventional Commits](https://www.conventionalcommits.org/).
- No magic bitboards. Slider attacks are bitscan rays over `occupancy & !jump`.
- No this-ply freeze/jump reuse filter. `generate_turns_from` still calls `legal_moves` per relevant spell target.
- `generate_turns` stays exhaustive; `generate_search_turns` stays relevance-filtered. Semantics unchanged.
- Mailbox `get`/`set` stay. Eval, ordering, and the CLI keep calling `board.get`.
- `crates/cli` is untouched.
- Production `legal_moves` must match `legal_moves_reference` exactly on the fuzz battery, including freeze/jump hypotheticals. A mismatch is a failed test, not a performance tradeoff.
- Depth-3 success bar: under 5 seconds on this machine, release, starting position. If measurement misses, stop and report — do not silently add the reuse filter.

## File map

| File | Responsibility |
|---|---|
| Create `crates/core/src/bitboard.rs` | `Bitboard(u64)` newtype, bit ops, square iterator |
| Modify `crates/core/src/types.rs` | `Color::index`, `PieceKind::index` |
| Modify `crates/core/src/lib.rs` | `mod bitboard`; export `Bitboard` |
| Modify `crates/core/src/board.rs` | Hybrid mailbox + bitboards + king cache; `Copy`; `set` is the only writer |
| Modify `crates/core/src/position.rs` | `Position: Copy` |
| Modify `crates/core/src/spells.rs` | `FREEZE_ZONE`, `frozen_bb`, `jump_bb`; bit-test `is_square_frozen` / `is_square_jump_active` |
| Modify `crates/core/src/rays.rs` | Attack tables, `ray_attacks`; `walk_ray` becomes an ordered helper on the same occupancy |
| Modify `crates/core/src/attacks.rs` | `is_square_attacked` / `attacker_count` from piece bitboards |
| Modify `crates/core/src/movegen.rs` | Iterate `own & !frozen`; sliders via `ray_attacks` |
| Modify `crates/core/src/legal.rs` | Checkers/pins `legal_moves` |
| Create `crates/core/tests/legal_moves_soundness.rs` | Reference clone-and-rescan + differential fuzz |
| Modify `crates/search/src/search.rs` | Ignored depth-1 / depth-3 timing tests |

---

### Task 1: `Bitboard` newtype and piece/color indices

**Files:**
- Create: `crates/core/src/bitboard.rs`
- Modify: `crates/core/src/types.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Produces: `spellchess_core::Bitboard` with `EMPTY`, `from_square`, `contains`, `with`/`without` (immutable set/clear), `union`, `intersect`, `minus`, `is_empty`, `count`, `iter() -> impl Iterator<Item = Square>`.
- Produces: `Color::index(self) -> usize` (White=0, Black=1), `PieceKind::index(self) -> usize` (Pawn=0 … King=5).

- [ ] **Step 1: Write the failing `Bitboard` tests**

Create `crates/core/src/bitboard.rs` with tests first (the struct can be a stub so the file parses; methods the tests call should be missing or `unimplemented!` so they fail):

```rust
use crate::types::Square;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Bitboard(pub u64);

impl Bitboard {
    pub const EMPTY: Bitboard = Bitboard(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_square_sets_only_that_bit() {
        let bb = Bitboard::from_square(Square::from_str("e4").unwrap());
        assert!(bb.contains(Square::from_str("e4").unwrap()));
        assert!(!bb.contains(Square::from_str("e5").unwrap()));
        assert_eq!(bb.count(), 1);
    }

    #[test]
    fn a1_is_bit_zero() {
        let bb = Bitboard::from_square(Square::from_str("a1").unwrap());
        assert_eq!(bb.0, 1);
    }

    #[test]
    fn iter_yields_set_squares_in_index_order() {
        let mut bb = Bitboard::EMPTY;
        bb = bb.with(Square::from_str("h8").unwrap());
        bb = bb.with(Square::from_str("a1").unwrap());
        bb = bb.with(Square::from_str("e4").unwrap());
        let sqs: Vec<_> = bb.iter().collect();
        assert_eq!(sqs, vec![
            Square::from_str("a1").unwrap(),
            Square::from_str("e4").unwrap(),
            Square::from_str("h8").unwrap(),
        ]);
    }

    #[test]
    fn minus_clears_intersection() {
        let a = Bitboard::from_square(Square::from_str("a1").unwrap())
            .with(Square::from_str("b1").unwrap());
        let b = Bitboard::from_square(Square::from_str("b1").unwrap());
        let d = a.minus(b);
        assert!(d.contains(Square::from_str("a1").unwrap()));
        assert!(!d.contains(Square::from_str("b1").unwrap()));
    }
}
```

Add to `crates/core/src/types.rs` (in the existing `tests` module):

```rust
    #[test]
    fn color_and_piece_kind_indices_are_dense() {
        assert_eq!(Color::White.index(), 0);
        assert_eq!(Color::Black.index(), 1);
        assert_eq!(PieceKind::Pawn.index(), 0);
        assert_eq!(PieceKind::Knight.index(), 1);
        assert_eq!(PieceKind::Bishop.index(), 2);
        assert_eq!(PieceKind::Rook.index(), 3);
        assert_eq!(PieceKind::Queen.index(), 4);
        assert_eq!(PieceKind::King.index(), 5);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p spellchess-core bitboard:: --lib`
Expected: compile error (`from_square` / `index` not found) or FAIL.

- [ ] **Step 3: Implement `Bitboard` and indices**

`crates/core/src/bitboard.rs` (replace the stub):

```rust
use crate::types::Square;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Bitboard(pub u64);

impl Bitboard {
    pub const EMPTY: Bitboard = Bitboard(0);

    pub fn from_square(sq: Square) -> Bitboard {
        Bitboard(1u64 << sq.0)
    }

    pub fn contains(self, sq: Square) -> bool {
        (self.0 >> sq.0) & 1 == 1
    }

    pub fn with(self, sq: Square) -> Bitboard {
        Bitboard(self.0 | (1u64 << sq.0))
    }

    pub fn without(self, sq: Square) -> Bitboard {
        Bitboard(self.0 & !(1u64 << sq.0))
    }

    pub fn union(self, other: Bitboard) -> Bitboard {
        Bitboard(self.0 | other.0)
    }

    pub fn intersect(self, other: Bitboard) -> Bitboard {
        Bitboard(self.0 & other.0)
    }

    pub fn minus(self, other: Bitboard) -> Bitboard {
        Bitboard(self.0 & !other.0)
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn count(self) -> u32 {
        self.0.count_ones()
    }

    pub fn iter(self) -> BitIter {
        BitIter(self.0)
    }
}

pub struct BitIter(u64);

impl Iterator for BitIter {
    type Item = Square;
    fn next(&mut self) -> Option<Square> {
        if self.0 == 0 {
            return None;
        }
        let tz = self.0.trailing_zeros() as u8;
        self.0 &= self.0.wrapping_sub(1);
        Some(Square(tz))
    }
}
```

In `crates/core/src/types.rs`, add to `impl Color`:

```rust
    pub fn index(self) -> usize {
        match self {
            Color::White => 0,
            Color::Black => 1,
        }
    }
```

Add to `PieceKind`:

```rust
impl PieceKind {
    pub fn index(self) -> usize {
        match self {
            PieceKind::Pawn => 0,
            PieceKind::Knight => 1,
            PieceKind::Bishop => 2,
            PieceKind::Rook => 3,
            PieceKind::Queen => 4,
            PieceKind::King => 5,
        }
    }
}
```

In `crates/core/src/lib.rs`, after `pub use types::*;`:

```rust
pub mod bitboard;
pub use bitboard::Bitboard;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p spellchess-core --lib bitboard:: color_and_piece_kind_indices`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/bitboard.rs crates/core/src/types.rs crates/core/src/lib.rs
git commit -m "feat(core): add Bitboard newtype and color/kind indices"
```

---

### Task 2: Hybrid `Board`, `Copy` `Position`, freeze-zone table

**Files:**
- Modify: `crates/core/src/board.rs`
- Modify: `crates/core/src/position.rs`
- Modify: `crates/core/src/spells.rs` (add `FREEZE_ZONE` + tests; leave `freeze_zone` Vec helper in place for now)

**Interfaces:**
- Consumes: `Bitboard`, `Color::index`, `PieceKind::index`
- Produces: `Board` is `Copy`. Fields: `squares: [Option<Piece>; 64]`, `by_color: [Bitboard; 2]`, `by_kind: [Bitboard; 6]`, `kings: [Option<Square>; 2]`.
- Produces: `Board::occupancy(&self) -> Bitboard`, `Board::color_bb(&self, Color) -> Bitboard`, `Board::kind_bb(&self, PieceKind) -> Bitboard`. `get`/`set`/`king_square` keep their signatures. `set` is the only writer.
- Produces: `Position: Copy`.
- Produces: `spells::FREEZE_ZONE: [Bitboard; 64]` matching `freeze_zone` geometry.

- [ ] **Step 1: Write the failing tests**

Add to `crates/core/src/board.rs` tests:

```rust
    #[test]
    fn board_is_copy() {
        let a = Board::starting();
        let b = a;
        assert_eq!(a.king_square(Color::White), b.king_square(Color::White));
    }

    #[test]
    fn occupancy_matches_mailbox_on_starting_board() {
        let b = Board::starting();
        let mut mailbox = crate::bitboard::Bitboard::EMPTY;
        for i in 0..64u8 {
            if b.get(Square(i)).is_some() {
                mailbox = mailbox.with(Square(i));
            }
        }
        assert_eq!(b.occupancy(), mailbox);
        assert_eq!(b.occupancy().count(), 32);
    }

    #[test]
    fn set_none_clears_bits_and_king_cache() {
        let mut b = Board::starting();
        b.set(Square::from_str("e1").unwrap(), None);
        assert_eq!(b.king_square(Color::White), None);
        assert!(!b.occupancy().contains(Square::from_str("e1").unwrap()));
        assert_eq!(b.get(Square::from_str("e1").unwrap()), None);
    }

    #[test]
    fn set_piece_updates_color_and_kind_bits() {
        let mut b = Board::empty();
        let rook = Piece { color: Color::White, kind: PieceKind::Rook };
        b.set(Square::from_str("d4").unwrap(), Some(rook));
        assert!(b.color_bb(Color::White).contains(Square::from_str("d4").unwrap()));
        assert!(b.kind_bb(PieceKind::Rook).contains(Square::from_str("d4").unwrap()));
        assert!(!b.kind_bb(PieceKind::King).contains(Square::from_str("d4").unwrap()));
    }
```

Add to `crates/core/src/position.rs` tests:

```rust
    #[test]
    fn position_is_copy() {
        let a = Position::starting();
        let b = a;
        assert_eq!(a.side_to_move, b.side_to_move);
        assert_eq!(a.board.king_square(Color::White), b.board.king_square(Color::White));
    }
```

Add to `crates/core/src/spells.rs` tests:

```rust
    #[test]
    fn freeze_zone_table_matches_freeze_zone_for_every_square() {
        for i in 0..64u8 {
            let sq = Square(i);
            let mut from_vec = crate::bitboard::Bitboard::EMPTY;
            for z in freeze_zone(sq) {
                from_vec = from_vec.with(z);
            }
            assert_eq!(FREEZE_ZONE[i as usize], from_vec, "mismatch at {sq}");
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p spellchess-core --lib board_is_copy occupancy_matches position_is_copy freeze_zone_table`
Expected: compile error (`occupancy` / `FREEZE_ZONE` / `Copy` missing).

- [ ] **Step 3: Implement hybrid `Board`, `Copy` `Position`, `FREEZE_ZONE`**

Replace `Board` in `crates/core/src/board.rs`:

```rust
use crate::bitboard::Bitboard;
use crate::types::{Color, Piece, PieceKind, Square};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Board {
    squares: [Option<Piece>; 64],
    by_color: [Bitboard; 2],
    by_kind: [Bitboard; 6],
    kings: [Option<Square>; 2],
}

impl Board {
    pub fn empty() -> Board {
        Board {
            squares: [None; 64],
            by_color: [Bitboard::EMPTY; 2],
            by_kind: [Bitboard::EMPTY; 6],
            kings: [None; 2],
        }
    }

    pub fn starting() -> Board {
        let mut b = Board::empty();
        let back_rank = [
            PieceKind::Rook, PieceKind::Knight, PieceKind::Bishop, PieceKind::Queen,
            PieceKind::King, PieceKind::Bishop, PieceKind::Knight, PieceKind::Rook,
        ];
        for (file, kind) in back_rank.iter().enumerate() {
            b.set(Square::new(file as u8, 0), Some(Piece { color: Color::White, kind: *kind }));
            b.set(Square::new(file as u8, 7), Some(Piece { color: Color::Black, kind: *kind }));
        }
        for file in 0..8u8 {
            b.set(Square::new(file, 1), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
            b.set(Square::new(file, 6), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        }
        b
    }

    pub fn get(&self, sq: Square) -> Option<Piece> {
        self.squares[sq.0 as usize]
    }

    pub fn occupancy(&self) -> Bitboard {
        self.by_color[0].union(self.by_color[1])
    }

    pub fn color_bb(&self, color: Color) -> Bitboard {
        self.by_color[color.index()]
    }

    pub fn kind_bb(&self, kind: PieceKind) -> Bitboard {
        self.by_kind[kind.index()]
    }

    pub fn set(&mut self, sq: Square, piece: Option<Piece>) {
        if let Some(old) = self.squares[sq.0 as usize] {
            self.by_color[old.color.index()] = self.by_color[old.color.index()].without(sq);
            self.by_kind[old.kind.index()] = self.by_kind[old.kind.index()].without(sq);
            if old.kind == PieceKind::King {
                self.kings[old.color.index()] = None;
            }
        }
        self.squares[sq.0 as usize] = piece;
        if let Some(p) = piece {
            self.by_color[p.color.index()] = self.by_color[p.color.index()].with(sq);
            self.by_kind[p.kind.index()] = self.by_kind[p.kind.index()].with(sq);
            if p.kind == PieceKind::King {
                self.kings[p.color.index()] = Some(sq);
            }
        }
        #[cfg(debug_assertions)]
        self.assert_consistent();
    }

    pub fn king_square(&self, color: Color) -> Option<Square> {
        self.kings[color.index()]
    }

    fn assert_consistent(&self) {
        let mut by_color = [Bitboard::EMPTY; 2];
        let mut by_kind = [Bitboard::EMPTY; 6];
        let mut kings = [None; 2];
        for i in 0..64u8 {
            let sq = Square(i);
            if let Some(p) = self.squares[i as usize] {
                by_color[p.color.index()] = by_color[p.color.index()].with(sq);
                by_kind[p.kind.index()] = by_kind[p.kind.index()].with(sq);
                if p.kind == PieceKind::King {
                    kings[p.color.index()] = Some(sq);
                }
            }
        }
        debug_assert_eq!(self.by_color, by_color, "Board color bits desynced from mailbox");
        debug_assert_eq!(self.by_kind, by_kind, "Board kind bits desynced from mailbox");
        debug_assert_eq!(self.kings, kings, "Board king cache desynced from mailbox");
    }
}
```

Keep the existing `board.rs` tests (`starting_board_has_32_pieces`, etc.).

In `crates/core/src/position.rs`, change the `Position` derive to:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Position {
```

In `crates/core/src/spells.rs`, add (near `freeze_zone`):

```rust
use crate::bitboard::Bitboard;

const fn freeze_zone_bits(idx: u8) -> u64 {
    let tf = (idx % 8) as i8;
    let tr = (idx / 8) as i8;
    let mut bits = 0u64;
    let mut df = -1i8;
    while df <= 1 {
        let mut dr = -1i8;
        while dr <= 1 {
            let f = tf + df;
            let r = tr + dr;
            if f >= 0 && f < 8 && r >= 0 && r < 8 {
                bits |= 1u64 << (r * 8 + f) as u32;
            }
            dr += 1;
        }
        df += 1;
    }
    bits
}

pub const FREEZE_ZONE: [Bitboard; 64] = {
    let mut table = [Bitboard(0); 64];
    let mut i = 0;
    while i < 64 {
        table[i] = Bitboard(freeze_zone_bits(i as u8));
        i += 1;
    }
    table
};
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p spellchess-core`
Expected: PASS (workspace core tests, including `oracle_vectors` and `relevance_soundness`). Movegen is unchanged; only representation changed.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/board.rs crates/core/src/position.rs crates/core/src/spells.rs
git commit -m "perf(core): store occupancy bitboards and cached kings on Board"
```

---

### Task 3: Spell masks — `frozen_bb` / `jump_bb` and bit tests

**Files:**
- Modify: `crates/core/src/spells.rs`

**Interfaces:**
- Consumes: `FREEZE_ZONE`, `FieldSet` iteration, `field_active`
- Produces: `pub fn frozen_bb(pos: &Position) -> Bitboard`, `pub fn jump_bb(pos: &Position) -> Bitboard`. `is_square_frozen` / `is_square_jump_active` become `.contains` tests on those masks. `freeze_zone` Vec helper may remain for tests that call it directly.

- [ ] **Step 1: Write the failing tests**

Add to `crates/core/src/spells.rs` tests:

```rust
    #[test]
    fn frozen_bb_covers_the_3x3_and_not_beyond() {
        let mut pos = Position { board: crate::board::Board::empty(), ..Position::starting() };
        pos.fields.push(SpellField {
            square: Square::from_str("d5").unwrap(), owner: Color::White,
            kind: SpellKind::Freeze, expires_after_ply: pos.ply + 1,
        });
        let frozen = frozen_bb(&pos);
        assert!(frozen.contains(Square::from_str("d5").unwrap()));
        assert!(frozen.contains(Square::from_str("e6").unwrap()));
        assert!(!frozen.contains(Square::from_str("f7").unwrap()));
        assert_eq!(frozen.count(), 9);
    }

    #[test]
    fn jump_bb_is_exactly_the_anchor_square() {
        let mut pos = Position { board: crate::board::Board::empty(), ..Position::starting() };
        pos.fields.push(SpellField {
            square: Square::from_str("d4").unwrap(), owner: Color::White,
            kind: SpellKind::Jump, expires_after_ply: pos.ply + 1,
        });
        let jump = jump_bb(&pos);
        assert_eq!(jump, Bitboard::from_square(Square::from_str("d4").unwrap()));
        assert!(is_square_jump_active(&pos, Square::from_str("d4").unwrap()));
        assert!(!is_square_jump_active(&pos, Square::from_str("d5").unwrap()));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p spellchess-core --lib frozen_bb_covers jump_bb_is_exactly`
Expected: compile error (`frozen_bb` / `jump_bb` not found).

- [ ] **Step 3: Implement masks and switch the predicates**

```rust
pub fn frozen_bb(pos: &Position) -> Bitboard {
    let mut acc = Bitboard::EMPTY;
    for f in pos.fields.iter() {
        if f.kind == SpellKind::Freeze && field_active(pos, f) {
            acc = acc.union(FREEZE_ZONE[f.square.0 as usize]);
        }
    }
    acc
}

pub fn jump_bb(pos: &Position) -> Bitboard {
    let mut acc = Bitboard::EMPTY;
    for f in pos.fields.iter() {
        if f.kind == SpellKind::Jump && field_active(pos, f) {
            acc = acc.with(f.square);
        }
    }
    acc
}

pub fn is_square_frozen(pos: &Position, square: Square) -> bool {
    frozen_bb(pos).contains(square)
}

pub fn is_square_jump_active(pos: &Position, square: Square) -> bool {
    jump_bb(pos).contains(square)
}
```

Delete the old `is_square_frozen` body that called `freeze_zone(f.square).contains`. Keep `is_field_anchor` as a field-anchor equality check (recast exclusion) — that is not a zone test.

- [ ] **Step 4: Run tests**

Run: `cargo test -p spellchess-core`
Expected: PASS. `vector_4_frozen_piece_exerts_no_control` and freeze-zone clipping tests still pass.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/spells.rs
git commit -m "perf(core): derive frozen and jump occupancy from bitboard masks"
```

---

### Task 4: Bitboard attacks — tables, bitscan rays, rewrite `is_square_attacked`

**Files:**
- Modify: `crates/core/src/rays.rs`
- Modify: `crates/core/src/attacks.rs`

**Interfaces:**
- Consumes: `Board::occupancy` / `color_bb` / `kind_bb`, `frozen_bb`, `jump_bb`
- Produces: `rays::KNIGHT_ATTACKS: [Bitboard; 64]`, `KING_ATTACKS: [Bitboard; 64]`, `PAWN_ATTACKS: [[Bitboard; 64]; 2]` (index by `Color::index`).
- Produces: `pub fn ray_attacks(from: Square, occ: Bitboard, dir: (i8, i8)) -> Bitboard` — includes the first occupied square, then stops. Caller passes `occupancy.minus(jump)`.
- Produces: `pub fn rook_attacks(from: Square, occ: Bitboard) -> Bitboard`, `bishop_attacks(from: Square, occ: Bitboard) -> Bitboard`.
- `walk_ray` stays as an ordered `Vec<Square>` helper used by `spells.rs` relevance (first/second blocker). Reimplement it by walking the same occupancy as `ray_attacks` so `.last()` remains the first non-jump blocker.
- `is_square_attacked` / `attacker_count` query attacks **from pieces**: leapers via tables intersected with unfrozen enemy pieces; sliders via `rook_attacks`/`bishop_attacks(square, occupancy.minus(jump))` intersected with unfrozen enemy rook/bishop/queen bits. A slider standing on a jump square still counts — its origin is in the attack set because jump occupancy does not treat that origin as a blocker for the *target-side* walk; equivalently, generating from the piece's origin never treats the origin as a blocker.

- [ ] **Step 1: Write the failing tests**

Add to `crates/core/src/rays.rs` tests:

```rust
    #[test]
    fn knight_attacks_from_e4_are_the_eight_leaps() {
        let bb = KNIGHT_ATTACKS[Square::from_str("e4").unwrap().0 as usize];
        assert_eq!(bb.count(), 8);
        assert!(bb.contains(Square::from_str("d6").unwrap()));
        assert!(bb.contains(Square::from_str("f2").unwrap()));
        assert!(!bb.contains(Square::from_str("e5").unwrap()));
    }

    #[test]
    fn ray_attacks_stop_at_first_occupied_and_include_it() {
        let occ = Bitboard::from_square(Square::from_str("f4").unwrap());
        let ray = ray_attacks(Square::from_str("d4").unwrap(), occ, (1, 0));
        assert!(ray.contains(Square::from_str("e4").unwrap()));
        assert!(ray.contains(Square::from_str("f4").unwrap()));
        assert!(!ray.contains(Square::from_str("g4").unwrap()));
    }

    #[test]
    fn ray_attacks_see_through_a_square_cleared_from_occupancy() {
        // Jump modelling: the jumped piece is removed from slider occupancy.
        let occ = Bitboard::EMPTY;
        let ray = ray_attacks(Square::from_str("d4").unwrap(), occ, (1, 0));
        assert!(ray.contains(Square::from_str("h4").unwrap()));
    }
```

Keep existing `walk_ray` tests; they must still pass after the helper rewrite.

Add to `crates/core/src/attacks.rs` tests:

```rust
    #[test]
    fn slider_on_a_jump_square_still_attacks() {
        // Walk-from-target + last() used to see through the jumper and miss it.
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("d4").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.fields.push(crate::position::SpellField {
            square: Square::from_str("d4").unwrap(), owner: Color::White,
            kind: crate::position::SpellKind::Jump, expires_after_ply: pos.ply + 1,
        });
        assert!(is_square_attacked(&pos, Square::from_str("d8").unwrap(), Color::White));
        assert_eq!(attacker_count(&pos, Square::from_str("d8").unwrap(), Color::White), 1);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p spellchess-core --lib knight_attacks_from_e4 ray_attacks_stop slider_on_a_jump`
Expected: compile error or FAIL.

- [ ] **Step 3: Implement tables, `ray_attacks`, rewrite attacks**

In `crates/core/src/rays.rs`, add tables and bitscan. Build knight/king/pawn tables in a `const fn` or a `OnceLock` — prefer a `const fn` fill so there is no runtime init. Sketch:

```rust
use crate::bitboard::Bitboard;
use crate::types::{Color, Square};

pub const ROOK_DIRS: [(i8, i8); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
pub const BISHOP_DIRS: [(i8, i8); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];
const KNIGHT_OFFSETS: [(i8, i8); 8] = [(1, 2), (2, 1), (2, -1), (1, -2), (-1, -2), (-2, -1), (-2, 1), (-1, 2)];
const KING_OFFSETS: [(i8, i8); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];

const fn bit_at(file: i8, rank: i8) -> u64 {
    if file >= 0 && file < 8 && rank >= 0 && rank < 8 {
        1u64 << (rank * 8 + file) as u32
    } else {
        0
    }
}

const fn fill_leaper(offsets: &[(i8, i8); 8]) -> [Bitboard; 64] {
    let mut table = [Bitboard(0); 64];
    let mut i = 0;
    while i < 64 {
        let file = (i % 8) as i8;
        let rank = (i / 8) as i8;
        let mut bits = 0u64;
        let mut k = 0;
        while k < 8 {
            bits |= bit_at(file + offsets[k].0, rank + offsets[k].1);
            k += 1;
        }
        table[i] = Bitboard(bits);
        i += 1;
    }
    table
}

pub const KNIGHT_ATTACKS: [Bitboard; 64] = fill_leaper(&KNIGHT_OFFSETS);
pub const KING_ATTACKS: [Bitboard; 64] = fill_leaper(&KING_OFFSETS);

const fn fill_pawn(white: bool) -> [Bitboard; 64] {
    let dir: i8 = if white { 1 } else { -1 };
    let mut table = [Bitboard(0); 64];
    let mut i = 0;
    while i < 64 {
        let file = (i % 8) as i8;
        let rank = (i / 8) as i8;
        let mut bits = 0u64;
        bits |= bit_at(file - 1, rank + dir);
        bits |= bit_at(file + 1, rank + dir);
        table[i] = Bitboard(bits);
        i += 1;
    }
    table
}

pub const PAWN_ATTACKS: [[Bitboard; 64]; 2] = [fill_pawn(true), fill_pawn(false)];

pub fn ray_attacks(from: Square, occ: Bitboard, dir: (i8, i8)) -> Bitboard {
    let mut bits = 0u64;
    let mut f = from.file() as i8 + dir.0;
    let mut r = from.rank() as i8 + dir.1;
    while (0..8).contains(&f) && (0..8).contains(&r) {
        let idx = (r * 8 + f) as u32;
        bits |= 1u64 << idx;
        if occ.contains(Square::new(f as u8, r as u8)) {
            break;
        }
        f += dir.0;
        r += dir.1;
    }
    Bitboard(bits)
}

pub fn rook_attacks(from: Square, occ: Bitboard) -> Bitboard {
    let mut acc = Bitboard::EMPTY;
    for dir in ROOK_DIRS {
        acc = acc.union(ray_attacks(from, occ, dir));
    }
    acc
}

pub fn bishop_attacks(from: Square, occ: Bitboard) -> Bitboard {
    let mut acc = Bitboard::EMPTY;
    for dir in BISHOP_DIRS {
        acc = acc.union(ray_attacks(from, occ, dir));
    }
    acc
}

pub fn walk_ray(pos: &crate::position::Position, from: Square, dir: (i8, i8)) -> Vec<Square> {
    let occ = pos.board.occupancy().minus(crate::spells::jump_bb(pos));
    let mut out = Vec::new();
    let mut f = from.file() as i8 + dir.0;
    let mut r = from.rank() as i8 + dir.1;
    while (0..8).contains(&f) && (0..8).contains(&r) {
        let sq = Square::new(f as u8, r as u8);
        out.push(sq);
        if occ.contains(sq) {
            break;
        }
        f += dir.0;
        r += dir.1;
    }
    out
}
```

Replace `is_square_attacked` / `attacker_count` in `crates/core/src/attacks.rs`:

```rust
use crate::bitboard::Bitboard;
use crate::position::Position;
use crate::rays::{bishop_attacks, rook_attacks, KING_ATTACKS, KNIGHT_ATTACKS, PAWN_ATTACKS};
use crate::types::{Color, PieceKind, Square};

fn slider_occ(pos: &Position) -> Bitboard {
    pos.board.occupancy().minus(crate::spells::jump_bb(pos))
}

fn unfrozen(pos: &Position, color: Color, kind: PieceKind) -> Bitboard {
    pos.board.color_bb(color)
        .intersect(pos.board.kind_bb(kind))
        .minus(crate::spells::frozen_bb(pos))
}

fn collect_attackers(pos: &Position, square: Square, by: Color, stop_at_one: bool) -> u32 {
    let mut count = 0u32;
    let occ = slider_occ(pos);
    let idx = square.0 as usize;
    // Reverse pawn attacks: PAWN_ATTACKS[White][from] is NE/NW of from, so the white
    // pawns that attack `square` sit on PAWN_ATTACKS[Black][square], and vice versa.
    count += unfrozen(pos, by, PieceKind::Pawn)
        .intersect(PAWN_ATTACKS[by.opposite().index()][idx])
        .count();
    if stop_at_one && count > 0 { return count; }

    count += unfrozen(pos, by, PieceKind::Knight).intersect(KNIGHT_ATTACKS[idx]).count();
    if stop_at_one && count > 0 { return count; }

    count += unfrozen(pos, by, PieceKind::King).intersect(KING_ATTACKS[idx]).count();
    if stop_at_one && count > 0 { return count; }

    let bq = unfrozen(pos, by, PieceKind::Bishop).union(unfrozen(pos, by, PieceKind::Queen));
    count += bq.intersect(bishop_attacks(square, occ)).count();
    if stop_at_one && count > 0 { return count; }

    let rq = unfrozen(pos, by, PieceKind::Rook).union(unfrozen(pos, by, PieceKind::Queen));
    count += rq.intersect(rook_attacks(square, occ)).count();
    count
}

pub fn is_square_attacked(pos: &Position, square: Square, by: Color) -> bool {
    collect_attackers(pos, square, by, true) > 0
}

pub fn attacker_count(pos: &Position, square: Square, by: Color) -> u32 {
    collect_attackers(pos, square, by, false)
}
```

Keep existing attack unit tests (`rook_attacks_along_clear_file`, `vector_4_frozen_piece_exerts_no_control`).

- [ ] **Step 4: Run tests**

Run: `cargo test -p spellchess-core`
Expected: PASS, including `slider_on_a_jump_square_still_attacks` and `oracle_vectors`.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/rays.rs crates/core/src/attacks.rs
git commit -m "perf(core): generate attacks from bitboards with jump occupancy"
```

---

### Task 5: Bitboard `pseudo_legal_moves`

**Files:**
- Modify: `crates/core/src/movegen.rs`

**Interfaces:**
- Consumes: `color_bb`, `frozen_bb`, `jump_bb`, `KNIGHT_ATTACKS`, `KING_ATTACKS`, `rook_attacks`, `bishop_attacks`, `ray_attacks`
- Produces: same `pseudo_legal_moves(pos: &Position) -> Vec<PieceMove>` signature and the same move set as today.

- [ ] **Step 1: Characterization — existing movegen tests must stay the contract**

Do not rewrite tests. The existing `movegen.rs` tests plus `oracle_vectors` destinations are the spec. After the rewrite they must pass unchanged.

- [ ] **Step 2: Rewrite generation to iterate `own & !frozen`**

Replace the `for i in 0..64` loop in `pseudo_legal_moves` with:

```rust
pub fn pseudo_legal_moves(pos: &Position) -> Vec<PieceMove> {
    let color = pos.side_to_move;
    let frozen = crate::spells::frozen_bb(pos);
    let jump = crate::spells::jump_bb(pos);
    let occ = pos.board.occupancy();
    let own = pos.board.color_bb(color);
    let enemy = pos.board.color_bb(color.opposite());
    let slider_occ = occ.minus(jump);
    let mut out = Vec::new();
    out.extend(castle_moves(pos, color));
    for sq in own.minus(frozen).iter() {
        let piece = pos.board.get(sq).expect("color bit set");
        match piece.kind {
            PieceKind::Pawn => out.extend(pawn_moves(pos, sq, color)),
            PieceKind::Knight => {
                for dest in KNIGHT_ATTACKS[sq.0 as usize].minus(own).iter() {
                    out.push(PieceMove::quiet(sq, dest));
                }
            }
            PieceKind::King => {
                for dest in KING_ATTACKS[sq.0 as usize].minus(own).iter() {
                    out.push(PieceMove::quiet(sq, dest));
                }
            }
            PieceKind::Bishop => out.extend(slide_dests(sq, own, slider_occ, true, false)),
            PieceKind::Rook => out.extend(slide_dests(sq, own, slider_occ, false, true)),
            PieceKind::Queen => out.extend(slide_dests(sq, own, slider_occ, true, true)),
        }
    }
    out
}

fn slide_dests(from: Square, own: Bitboard, slider_occ: Bitboard, bishop: bool, rook: bool) -> Vec<PieceMove> {
    let mut attacks = Bitboard::EMPTY;
    if bishop {
        attacks = attacks.union(crate::rays::bishop_attacks(from, slider_occ));
    }
    if rook {
        attacks = attacks.union(crate::rays::rook_attacks(from, slider_occ));
    }
    attacks.minus(own).iter().map(|to| PieceMove::quiet(from, to)).collect()
}
```

Keep `pawn_moves` and `castle_moves` as they are, except pawn double-step mid-square should use `jump.contains(mid)` instead of `is_square_jump_active` if that is already equivalent (it is, after Task 3). Prefer `jump_bb` already computed in `pseudo_legal_moves` by threading `jump: Bitboard` into `pawn_moves` — or leave `pawn_moves` calling `is_square_jump_active` for a smaller diff. Either is correct; threading is better.

Castling still uses `is_square_attacked` and mailbox rook checks. Do not change castling rules.

Delete `leaper_moves` / `slide_moves` if they become unused.

- [ ] **Step 3: Run tests**

Run: `cargo test -p spellchess-core`
Expected: PASS. In particular `lone_rook_on_d4_has_fourteen_moves`, `start_position_knight_g1_has_two_jumps`, `vector_11` pawn double-step over jump, `oracle_vectors`.

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/movegen.rs
git commit -m "perf(core): generate pseudo-legal moves from occupancy bitboards"
```

---

### Task 6: Checkers/pins `legal_moves` plus test-only reference

**Files:**
- Modify: `crates/core/src/legal.rs`
- Modify: `crates/core/src/rays.rs` (add `between` if not already there)
- Create: `crates/core/tests/legal_moves_soundness.rs` (reference helper lives here so it is not in the production crate)

**Interfaces:**
- Consumes: attack tables, `frozen_bb`, `jump_bb`, `attacker_count`, `apply_move_only`, `pseudo_legal_moves`
- Produces: production `legal_moves` with the spec’s filter order. Test-only `legal_moves_reference` in the integration test file, clone-and-rescan using public `pseudo_legal_moves` + `apply_move_only` + `is_square_attacked` / `attacker_count`.

- [ ] **Step 1: Write `legal_moves_reference` and a characterization test that today’s `legal_moves` already matches it**

Create `crates/core/tests/legal_moves_soundness.rs`:

```rust
use spellchess_core::*;

fn legal_moves_reference(pos: &Position) -> Vec<PieceMove> {
    let mover = pos.side_to_move;
    pseudo_legal_moves(pos)
        .into_iter()
        .filter(|mv| {
            let after = apply_move_only(pos, mv);
            if after.board.king_square(mover.opposite()).is_none() {
                let before_sq = pos.board.king_square(mover).expect("mover's king");
                let after_sq = after.board.king_square(mover).expect("mover's king after");
                let before_count = attacker_count(pos, before_sq, mover.opposite());
                let after_count = attacker_count(&after, after_sq, mover.opposite());
                return after_count <= before_count.max(1);
            }
            match after.board.king_square(mover) {
                Some(king_sq) => !is_square_attacked(&after, king_sq, mover.opposite()),
                None => true,
            }
        })
        .collect()
}

fn sort_key(mv: &PieceMove) -> (u8, u8, u8, bool, bool) {
    (mv.from.0, mv.to.0, mv.promotion.map(|p| p as u8).unwrap_or(255), mv.is_en_passant, mv.is_castle)
}

fn sorted(moves: &[PieceMove]) -> Vec<(u8, u8, u8, bool, bool)> {
    let mut v: Vec<_> = moves.iter().map(sort_key).collect();
    v.sort();
    v
}

#[test]
fn production_matches_reference_on_the_starting_position() {
    let pos = Position::starting();
    assert_eq!(sorted(&legal_moves(&pos)), sorted(&legal_moves_reference(&pos)));
}
```

`pub mod attacks` does not glob into `use spellchess_core::*`. Add to `crates/core/src/lib.rs` in this step:

```rust
pub use attacks::{attacker_count, is_square_attacked};
```

Then the reference can call `attacker_count(...)` directly.

- [ ] **Step 2: Run the characterization test**

Run: `cargo test -p spellchess-core --test legal_moves_soundness`
Expected: PASS against today’s clone-and-rescan `legal_moves` (they are the same algorithm).

- [ ] **Step 3: Add `between` and pin/checker helpers, then rewrite `legal_moves`**

In `crates/core/src/rays.rs`:

```rust
pub fn between(a: Square, b: Square) -> Bitboard {
    let df = b.file() as i8 - a.file() as i8;
    let dr = b.rank() as i8 - a.rank() as i8;
    if df == 0 && dr == 0 {
        return Bitboard::EMPTY;
    }
    let on_diag = df.abs() == dr.abs();
    let on_ortho = df == 0 || dr == 0;
    if !on_diag && !on_ortho {
        return Bitboard::EMPTY;
    }
    let step_f = df.signum();
    let step_r = dr.signum();
    let occ_stop = Bitboard::from_square(b);
    let ray = ray_attacks(a, occ_stop, (step_f, step_r));
    ray.without(b)
}
```

In `crates/core/src/legal.rs`, replace `legal_moves` (keep `apply_move_only` / turn generation unchanged):

```rust
pub fn legal_moves(pos: &Position) -> Vec<PieceMove> {
    let mover = pos.side_to_move;
    let enemy = mover.opposite();
    let Some(king_sq) = pos.board.king_square(mover) else {
        return Vec::new();
    };
    let frozen = crate::spells::frozen_bb(pos);
    let jump = crate::spells::jump_bb(pos);
    let occ = pos.board.occupancy();
    let slider_occ = occ.minus(jump);
    let checkers = checkers_of(pos, king_sq, enemy, frozen, slider_occ);
    let checker_count = checkers.count();
    let pins = pins_of(pos, king_sq, mover, frozen, slider_occ);

    pseudo_legal_moves(pos)
        .into_iter()
        .filter(|mv| {
            let dest_piece = pos.board.get(mv.to);
            if dest_piece.is_some_and(|p| p.kind == PieceKind::King && p.color == enemy) {
                let after = apply_move_only(pos, mv);
                let after_sq = after.board.king_square(mover).expect("own king remains");
                let before_count = crate::attacks::attacker_count(pos, king_sq, enemy);
                let after_count = crate::attacks::attacker_count(&after, after_sq, enemy);
                return after_count <= before_count.max(1);
            }
            if mv.is_en_passant {
                let after = apply_move_only(pos, mv);
                return !crate::attacks::is_square_attacked(&after, after.board.king_square(mover).unwrap(), enemy);
            }
            let mover_piece = pos.board.get(mv.from).unwrap();
            if mover_piece.kind == PieceKind::King {
                return king_dest_safe(pos, king_sq, mv.to, enemy);
            }
            if checker_count >= 2 {
                return false;
            }
            if let Some(ray) = pin_ray(&pins, mv.from) {
                if !ray.contains(mv.to) {
                    return false;
                }
            }
            if checker_count == 1 {
                let checker = checkers.iter().next().unwrap();
                return evasion_allows(pos, king_sq, checker, mv.to, jump);
            }
            true
        })
        .collect()
}

fn checkers_of(pos: &Position, king: Square, by: Color, frozen: Bitboard, slider_occ: Bitboard) -> Bitboard {
    let idx = king.0 as usize;
    let mut acc = Bitboard::EMPTY;
    let pawns = pos.board.color_bb(by).intersect(pos.board.kind_bb(PieceKind::Pawn)).minus(frozen);
    acc = acc.union(pawns.intersect(crate::rays::PAWN_ATTACKS[by.opposite().index()][idx]));
    let knights = pos.board.color_bb(by).intersect(pos.board.kind_bb(PieceKind::Knight)).minus(frozen);
    acc = acc.union(knights.intersect(crate::rays::KNIGHT_ATTACKS[idx]));
    let king_bb = pos.board.color_bb(by).intersect(pos.board.kind_bb(PieceKind::King)).minus(frozen);
    acc = acc.union(king_bb.intersect(crate::rays::KING_ATTACKS[idx]));
    let bq = pos.board.color_bb(by).intersect(pos.board.kind_bb(PieceKind::Bishop).union(pos.board.kind_bb(PieceKind::Queen))).minus(frozen);
    acc = acc.union(bq.intersect(crate::rays::bishop_attacks(king, slider_occ)));
    let rq = pos.board.color_bb(by).intersect(pos.board.kind_bb(PieceKind::Rook).union(pos.board.kind_bb(PieceKind::Queen))).minus(frozen);
    acc = acc.union(rq.intersect(crate::rays::rook_attacks(king, slider_occ)));
    acc
}

struct PinMap {
    pinned: Bitboard,
    rays: [Bitboard; 64],
}

fn first_two_on_ray(king: Square, slider_occ: Bitboard, dir: (i8, i8)) -> (Option<Square>, Option<Square>) {
    let mut first = None;
    let mut f = king.file() as i8 + dir.0;
    let mut r = king.rank() as i8 + dir.1;
    while (0..8).contains(&f) && (0..8).contains(&r) {
        let sq = Square::new(f as u8, r as u8);
        if slider_occ.contains(sq) {
            if first.is_none() {
                first = Some(sq);
            } else {
                return (first, Some(sq));
            }
        }
        f += dir.0;
        r += dir.1;
    }
    (first, None)
}

fn pins_of(pos: &Position, king: Square, us: Color, frozen: Bitboard, slider_occ: Bitboard) -> PinMap {
    let mut pinned = Bitboard::EMPTY;
    let mut rays = [Bitboard::EMPTY; 64];
    let them = us.opposite();
    for dir in crate::rays::ROOK_DIRS.iter().chain(crate::rays::BISHOP_DIRS.iter()) {
        let (Some(blocker), Some(second)) = first_two_on_ray(king, slider_occ, *dir) else { continue };
        if pos.board.get(blocker).is_none_or(|p| p.color != us) {
            continue;
        }
        let Some(p) = pos.board.get(second) else { continue };
        if p.color != them || frozen.contains(second) {
            continue;
        }
        let slider_ok = match p.kind {
            PieceKind::Queen => true,
            PieceKind::Rook => dir.0 == 0 || dir.1 == 0,
            PieceKind::Bishop => dir.0 != 0 && dir.1 != 0,
            _ => false,
        };
        if !slider_ok {
            continue;
        }
        pinned = pinned.with(blocker);
        rays[blocker.0 as usize] = crate::rays::between(king, second).with(second);
    }
    PinMap { pinned, rays }
}

fn pin_ray(pins: &PinMap, from: Square) -> Option<Bitboard> {
    if pins.pinned.contains(from) {
        Some(pins.rays[from.0 as usize])
    } else {
        None
    }
}

fn evasion_allows(pos: &Position, king: Square, checker: Square, dest: Square, jump: Bitboard) -> bool {
    if dest == checker {
        return true;
    }
    let Some(piece) = pos.board.get(checker) else { return false };
    let contact = matches!(piece.kind, PieceKind::Knight | PieceKind::Pawn | PieceKind::King)
        || crate::rays::between(king, checker).is_empty();
    if contact {
        return false;
    }
    let between = crate::rays::between(king, checker);
    if !between.intersect(jump).is_empty() {
        return false;
    }
    between.contains(dest)
}

fn king_dest_safe(pos: &Position, king_from: Square, dest: Square, enemy: Color) -> bool {
    // Copy is cheap; clearing the king (and a captured piece on dest) lets
    // is_square_attacked x-ray the vacated square. Frozen/jump come from probe.fields.
    let mut probe = *pos;
    probe.board.set(king_from, None);
    if pos.board.get(dest).is_some() {
        probe.board.set(dest, None);
    }
    !crate::attacks::is_square_attacked(&probe, dest, enemy)
}
```

Do not call `is_square_attacked` on the original `pos` for king destinations. Pin detection walks outward with `first_two_on_ray` — never `bitboard.iter().next()`, which is lowest square index, not closest on the ray.

- [ ] **Step 4: Run tests**

Run: `cargo test -p spellchess-core`
Expected: PASS, including `legal_moves_soundness`, `oracle_vectors`, existing `legal.rs` vectors (jump-through check, freeze-the-checker, king capture, castling under freeze).

If any fail, fix `legal_moves` — do not weaken the reference.

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/legal.rs crates/core/src/rays.rs crates/core/tests/legal_moves_soundness.rs crates/core/src/lib.rs
git commit -m "perf(core): filter legal moves with precomputed checkers and pins"
```

---

### Task 7: Differential fuzz including freeze/jump hypotheticals

**Files:**
- Modify: `crates/core/tests/legal_moves_soundness.rs`

**Interfaces:**
- Consumes: `legal_moves_reference`, `generate_turns`, `apply_turn`, `freeze_targets` / `jump_targets` (or occupied squares + `FREEZE_ZONE` anchors)
- Produces: fuzz covering tens of thousands of positions, each compared with and without an extra freeze/jump field.

- [ ] **Step 1: Write the fuzz tests**

Append to `crates/core/tests/legal_moves_soundness.rs` (reuse the splitmix64 / `random_legal_walk` / `with_field` pattern from `crates/core/tests/relevance_soundness.rs` — copy those helpers into this file; do not import across integration tests):

```rust
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
        let turns = generate_turns(&pos);
        if turns.is_empty() {
            break;
        }
        let pick = (splitmix64(&mut state) as usize) % turns.len();
        pos = apply_turn(&pos, &turns[pick]);
        if pos.board.king_square(Color::White).is_none() || pos.board.king_square(Color::Black).is_none() {
            break;
        }
        out.push(pos);
    }
    out
}

fn with_field(pos: &Position, kind: SpellKind, square: Square) -> Position {
    let mut next = *pos;
    next.fields.push(SpellField {
        square,
        owner: pos.side_to_move,
        kind,
        expires_after_ply: pos.ply + 1,
    });
    next
}

fn assert_match(pos: &Position, label: &str) {
    let prod = sorted(&legal_moves(pos));
    let refer = sorted(&legal_moves_reference(pos));
    assert_eq!(prod, refer, "{label} at ply {} side {:?}", pos.ply, pos.side_to_move);
}

#[test]
fn legal_moves_matches_reference_on_random_walks_and_spell_hypos() {
    let mut positions = vec![Position::starting()];
    for seed in 1u64..64 {
        positions.extend(random_legal_walk(seed, 12));
    }
    for pos in &positions {
        assert_match(pos, "baseline");
        if pos.board.king_square(Color::White).is_none() || pos.board.king_square(Color::Black).is_none() {
            continue;
        }
        for i in 0..64u8 {
            let sq = Square(i);
            let hypo_f = with_field(pos, SpellKind::Freeze, sq);
            assert_match(&hypo_f, "freeze hypo");
            if pos.board.get(sq).is_some() {
                let hypo_j = with_field(pos, SpellKind::Jump, sq);
                assert_match(&hypo_j, "jump hypo");
            }
        }
    }
}
```

64 seeds × 12 plies × (1 + 64 freeze + ~32 jump) is a large test. If it is too slow in debug, keep 16 seeds × 8 plies in the default test and add `#[ignore]` for a denser run (`cargo test -p spellchess-core --test legal_moves_soundness -- --ignored`). The default run must still cover thousands of positions including hypotheticals. Target: at least ~16 walks of 8 plies plus freeze/jump hypotheticals on each, which is well into the tens of thousands of `legal_moves` comparisons.

Also add a focused jump-through / pin case in this file (not only fuzz):

```rust
#[test]
fn jump_through_check_cannot_be_blocked_by_a_rook() {
    // Vector 12 geometry: Bb4 checks Ke1 through jump@d2; Ra1 has no legal moves.
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(Square::from_str("d2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Bishop }));
    pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(Square::from_str("h2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(Square::from_str("b4").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Bishop }));
    pos.fields.push(SpellField {
        square: Square::from_str("d2").unwrap(), owner: Color::Black,
        kind: SpellKind::Jump, expires_after_ply: pos.ply + 1,
    });
    assert_eq!(sorted(&legal_moves(&pos)), sorted(&legal_moves_reference(&pos)));
    let a1: Vec<_> = legal_moves(&pos).into_iter().filter(|m| m.from == Square::from_str("a1").unwrap()).collect();
    assert!(a1.is_empty());
}
```

- [ ] **Step 2: Run the fuzz**

Run: `cargo test -p spellchess-core --test legal_moves_soundness`
Expected: PASS. If a mismatch fires, **stop** — do not proceed to Task 8. Fix `legal_moves` (or the pin/evasion helpers) until the reference agrees. Trust the reference.

- [ ] **Step 3: Re-run the full core suite**

Run: `cargo test -p spellchess-core`
Expected: PASS (`oracle_vectors`, `relevance_soundness`, unit tests).

- [ ] **Step 4: Commit**

```bash
git add crates/core/tests/legal_moves_soundness.rs
git commit -m "test(core): differential-fuzz legal_moves against clone-and-rescan"
```

This is the correctness gate. Nothing after it proceeds on a red fuzz.

---

### Task 8: Remeasure search and tighten ignored timing tests

**Files:**
- Modify: `crates/search/src/search.rs` (ignored tests only)
- No production search-algorithm changes.

**Interfaces:**
- Consumes: unchanged `generate_search_turns`
- Produces: updated ignored depth-1 bound; new ignored depth-3 bound of 5 seconds.

- [ ] **Step 1: Remeasure on this machine**

Run, from the repo root:

```bash
cargo build -p spellchess-cli --release
/usr/bin/time -f '%e seconds' ./target/release/spellchess go --depth 1
/usr/bin/time -f '%e seconds' ./target/release/spellchess go --depth 2
/usr/bin/time -f '%e seconds' ./target/release/spellchess go --depth 3
```

If the CLI has no one-shot `go --depth N` that exits, use the existing ignored test as the clock:

```bash
cargo test -p spellchess-search --release -- --ignored --nocapture depth_budget
```

and a new depth-3 test below. Record wall-clock for depth 1/2/3 in the commit message.

If the CLI is a REPL, drive it with:

```bash
printf 'go --depth 1\nquit\n' | /usr/bin/time -f '%e seconds' ./target/release/spellchess
```

(Adjust to whatever the CLI actually reads — see `crates/cli/src/repl.rs`.)

- [ ] **Step 2: Write the failing depth-3 bound (and tighten depth 1)**

In `crates/search/src/search.rs` tests, update `depth_budget_stays_bounded_on_a_realistic_board`:

- Keep it `#[ignore]`.
- Change the depth-1 bound from 15s to **1 second** (spec: tens of milliseconds; 1s is a still-generous regression guard on a Pi 5).
- Update the doc comment to cite the new numbers and the spec path.

Add:

```rust
    #[test]
    #[ignore = "slow and misleading in a debug build; see depth_budget_stays_bounded_on_a_realistic_board"]
    fn depth3_budget_stays_bounded_on_a_realistic_board() {
        let pos = Position::starting();
        let start = std::time::Instant::now();
        let result = search(&pos, Budget::Depth(3));
        let elapsed = start.elapsed();
        assert!(result.is_some(), "a legal turn exists in the starting position");
        assert!(
            elapsed < Duration::from_secs(5),
            "depth-3 search on the starting position must finish in under 5s, took {elapsed:?}",
        );
    }
```

- [ ] **Step 3: Run ignored release tests**

Run: `cargo test -p spellchess-search --release -- --ignored --nocapture`
Expected: both depth-1 and depth-3 tests PASS.

If depth 3 is still tens of seconds: **do not** implement the this-ply reuse filter in this plan. Leave the new test `#[ignore]` failing, record the numbers, and stop for a follow-up spec. Do not silently expand scope.

- [ ] **Step 4: Full workspace tests**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/search/src/search.rs
git commit -m "test(search): bound depth-3 starting-position search under 5s"
```

Put the measured depth 1/2/3 times in the commit body.

---

## Self-review (plan vs spec)

| Spec section | Task |
|---|---|
| Hybrid `Board` + `Copy` `Position` | Task 2 |
| `FREEZE_ZONE` / `frozen` / `jump` masks | Tasks 2–3 |
| Bitscan sliders, no magics | Task 4 |
| Attacks from pieces; slider-on-jump still attacks | Task 4 |
| Bitboard `pseudo_legal_moves`; own & !frozen | Task 5 |
| Checkers/pins; king-capture first; EP copy-and-recheck; jump-through no block | Task 6 |
| Test-only reference; fuzz with freeze/jump hypos | Tasks 6–7 |
| `generate_turns` / `generate_search_turns` shape unchanged | Tasks 6, 8 (no edits) |
| Depth 3 under 5s; fallback not in this pass | Task 8 |
| Eval/CLI/`board.get` unchanged | all tasks |
| Mailbox stays | Task 2 |
| Debug desync panic | Task 2 `assert_consistent` |

No magic bitboards, no reuse filter, no eval rewrite, no CLI changes.

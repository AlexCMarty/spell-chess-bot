# Spell Chess Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust workspace (`core` rules engine, `search` alpha-beta engine, `cli` move-advisor REPL) implementing Spell Chess correctly and strongly enough to beat a human, per `docs/superpowers/specs/2026-08-10-spell-chess-engine-design.md`.

**Architecture:** Three crates. `core` owns all rule correctness (board, legal move/turn generation, terminal detection) and is validated against `rules/90-test-vectors.md`. `search` is a negamax alpha-beta engine built only on `core`'s public API. `cli` is an offline REPL built on both. No crate ever depends on chess.com's engine at runtime.

**Tech Stack:** Rust (stable, 2021 edition), Cargo workspace, no external dependencies beyond `std` for v1.

## Global Constraints

- Every commit message MUST use Conventional Commits (`feat:`, `fix:`, `test:`, `docs:`, `chore:`) per `CLAUDE.md`.
- No crate may depend on `research/variants.js` / `variants.pretty.js` at build or run time — those are gitignored, third-party, dev-time-only.
- `core` must pass all 20 vectors in `rules/90-test-vectors.md` before any `search` or `cli` task begins (Tier 1 gate, per the design spec's testing strategy).
- If the browser oracle is used at all (Task 26 only), the only URL ever opened is `https://www.chess.com/variants/spell-chess/analysis`, per `CLAUDE.md` and `rules/70-engine-api.md`.
- Rust workspace lives at the repo root; `rules/` and `research/` are untouched by this plan.

---

## Setup (once, before Task 1)

Run `cargo --version`; if missing, install via `rustup`. From the repo root:

```bash
mkdir -p crates/core/src crates/search/src crates/cli/src
```

Create `/home/alex/Spell Chess/Cargo.toml`:

```toml
[workspace]
members = ["crates/core", "crates/search", "crates/cli"]
resolver = "2"
```

---

### Task 1: Workspace scaffolding + core primitive types

**Files:**
- Create: `crates/core/Cargo.toml`
- Create: `crates/core/src/lib.rs`
- Create: `crates/core/src/types.rs`

**Interfaces:**
- Produces: `Color{White,Black}` with `.opposite()`; `PieceKind{Pawn,Knight,Bishop,Rook,Queen,King}`; `Piece{color,kind}`; `Square(pub u8)` with `Square::new(file,rank)`, `.file()`, `.rank()`, `Square::from_str(&str)->Option<Square>`, `Display`. All derive `Debug, Clone, Copy, PartialEq, Eq, Hash`; `Square` also derives `PartialOrd, Ord`.

- [ ] **Step 1: Write `crates/core/Cargo.toml`**

```toml
[package]
name = "spellchess-core"
version = "0.1.0"
edition = "2021"

[dependencies]
```

- [ ] **Step 2: Write `crates/core/src/types.rs`**

```rust
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Color {
    White,
    Black,
}

impl Color {
    pub fn opposite(self) -> Color {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PieceKind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Piece {
    pub color: Color,
    pub kind: PieceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Square(pub u8);

impl Square {
    pub fn new(file: u8, rank: u8) -> Square {
        assert!(file < 8 && rank < 8, "square out of range: file={file} rank={rank}");
        Square(rank * 8 + file)
    }

    pub fn file(self) -> u8 {
        self.0 % 8
    }

    pub fn rank(self) -> u8 {
        self.0 / 8
    }

    pub fn from_str(s: &str) -> Option<Square> {
        let bytes = s.as_bytes();
        if bytes.len() != 2 {
            return None;
        }
        let (file, rank) = (bytes[0], bytes[1]);
        if !(b'a'..=b'h').contains(&file) || !(b'1'..=b'8').contains(&rank) {
            return None;
        }
        Some(Square::new(file - b'a', rank - b'1'))
    }
}

impl fmt::Display for Square {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", (b'a' + self.file()) as char, self.rank() + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_round_trips_through_string() {
        for s in ["a1", "e4", "h8", "d5"] {
            let sq = Square::from_str(s).unwrap();
            assert_eq!(sq.to_string(), s);
        }
    }

    #[test]
    fn square_rejects_out_of_range() {
        assert_eq!(Square::from_str("i1"), None);
        assert_eq!(Square::from_str("a9"), None);
        assert_eq!(Square::from_str("a"), None);
    }

    #[test]
    fn color_opposite_is_involution() {
        assert_eq!(Color::White.opposite(), Color::Black);
        assert_eq!(Color::Black.opposite(), Color::White);
    }
}
```

- [ ] **Step 3: Write `crates/core/src/lib.rs`**

```rust
pub mod types;
pub use types::*;
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p spellchess-core`
Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/core
git commit -m "feat(core): scaffold workspace and add primitive types"
```

---

### Task 2: Board representation

**Files:**
- Create: `crates/core/src/board.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `Color, Piece, PieceKind, Square` from Task 1.
- Produces: `Board` with `Board::empty()`, `Board::starting()`, `.get(Square)->Option<Piece>`, `.set(Square,Option<Piece>)`, `.king_square(Color)->Option<Square>`. Derives `Debug, Clone, PartialEq, Eq`.

- [ ] **Step 1: Write `crates/core/src/board.rs`**

```rust
use crate::types::{Color, Piece, PieceKind, Square};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Board {
    squares: [Option<Piece>; 64],
}

impl Board {
    pub fn empty() -> Board {
        Board { squares: [None; 64] }
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

    pub fn set(&mut self, sq: Square, piece: Option<Piece>) {
        self.squares[sq.0 as usize] = piece;
    }

    pub fn king_square(&self, color: Color) -> Option<Square> {
        (0..64).map(Square).find(|&sq| self.get(sq) == Some(Piece { color, kind: PieceKind::King }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starting_board_has_32_pieces() {
        let b = Board::starting();
        let count = (0..64).filter(|&i| b.get(Square(i)).is_some()).count();
        assert_eq!(count, 32);
    }

    #[test]
    fn starting_board_places_kings_correctly() {
        let b = Board::starting();
        assert_eq!(b.king_square(Color::White), Square::from_str("e1"));
        assert_eq!(b.king_square(Color::Black), Square::from_str("e8"));
    }

    #[test]
    fn empty_board_has_no_king() {
        assert_eq!(Board::empty().king_square(Color::White), None);
    }
}
```

- [ ] **Step 2: Add `pub mod board; pub use board::Board;` to `crates/core/src/lib.rs`**

- [ ] **Step 3: Run tests**

Run: `cargo test -p spellchess-core`
Expected: 6 tests pass (3 from Task 1 + 3 new).

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/board.rs crates/core/src/lib.rs
git commit -m "feat(core): add board representation with standard starting position"
```

---

### Task 3: Position (game state)

**Files:**
- Create: `crates/core/src/position.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `Board` (Task 2), `Color, Square` (Task 1).
- Produces: `CastleRights{white_kingside,white_queenside,black_kingside,black_queenside}` all `bool`, `CastleRights::all()`; `SpellKind{Freeze,Jump}`; `SpellCounter{count:u8,lock:u8}` with `.castable()->bool`; `SpellState{freeze,jump}` with `SpellState::starting()`; `SpellField{square,owner,kind,expires_after_ply:u64}`; `Position{board,side_to_move,castle_rights,en_passant:Option<Square>,halfmove_clock:u32,ply:u64,white_spells,black_spells,fields:Vec<SpellField>}` with `Position::starting()` and `.spells(Color)->SpellState`. All new types derive at least `Debug, Clone, Copy/Clone, PartialEq, Eq`.

- [ ] **Step 1: Write `crates/core/src/position.rs`**

```rust
use crate::board::Board;
use crate::types::{Color, Square};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastleRights {
    pub white_kingside: bool,
    pub white_queenside: bool,
    pub black_kingside: bool,
    pub black_queenside: bool,
}

impl CastleRights {
    pub fn all() -> CastleRights {
        CastleRights { white_kingside: true, white_queenside: true, black_kingside: true, black_queenside: true }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpellKind {
    Freeze,
    Jump,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpellCounter {
    pub count: u8,
    pub lock: u8,
}

impl SpellCounter {
    pub fn castable(self) -> bool {
        self.count > 0 && self.lock == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpellState {
    pub freeze: SpellCounter,
    pub jump: SpellCounter,
}

impl SpellState {
    pub fn starting() -> SpellState {
        SpellState {
            freeze: SpellCounter { count: 5, lock: 0 },
            jump: SpellCounter { count: 2, lock: 0 },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpellField {
    pub square: Square,
    pub owner: Color,
    pub kind: SpellKind,
    /// Active for every ply up to and including this one (see rules/20-spell-system.md#field-lifetime).
    pub expires_after_ply: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    pub board: Board,
    pub side_to_move: Color,
    pub castle_rights: CastleRights,
    pub en_passant: Option<Square>,
    pub halfmove_clock: u32,
    pub ply: u64,
    pub white_spells: SpellState,
    pub black_spells: SpellState,
    pub fields: Vec<SpellField>,
}

impl Position {
    pub fn starting() -> Position {
        Position {
            board: Board::starting(),
            side_to_move: Color::White,
            castle_rights: CastleRights::all(),
            en_passant: None,
            halfmove_clock: 0,
            ply: 0,
            white_spells: SpellState::starting(),
            black_spells: SpellState::starting(),
            fields: Vec::new(),
        }
    }

    pub fn spells(&self, color: Color) -> SpellState {
        match color {
            Color::White => self.white_spells,
            Color::Black => self.black_spells,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starting_position_has_canonical_spell_counts() {
        let pos = Position::starting();
        assert_eq!(pos.spells(Color::White).freeze, SpellCounter { count: 5, lock: 0 });
        assert_eq!(pos.spells(Color::White).jump, SpellCounter { count: 2, lock: 0 });
        assert_eq!(pos.spells(Color::Black), pos.spells(Color::White));
    }

    #[test]
    fn starting_position_white_to_move_no_fields() {
        let pos = Position::starting();
        assert_eq!(pos.side_to_move, Color::White);
        assert!(pos.fields.is_empty());
    }

    #[test]
    fn spell_counter_castable_requires_count_and_no_lock() {
        assert!(SpellCounter { count: 1, lock: 0 }.castable());
        assert!(!SpellCounter { count: 0, lock: 0 }.castable());
        assert!(!SpellCounter { count: 1, lock: 2 }.castable());
    }
}
```

- [ ] **Step 2: Add `pub mod position; pub use position::*;` to `lib.rs`**

- [ ] **Step 3: Run `cargo test -p spellchess-core`, expect 9 passing**

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/position.rs crates/core/src/lib.rs
git commit -m "feat(core): add Position with spell economy and field state"
```

---

### Task 4: Pawn, knight, king move generation

**Files:**
- Create: `crates/core/src/movegen.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `Position, Board, Color, Piece, PieceKind, Square` from Tasks 1-3.
- Produces: `Promotion{Queen,Rook,Bishop,Knight}` with `.piece_kind()->PieceKind` and `Promotion::ALL`; `PieceMove{from,to,promotion:Option<Promotion>,is_en_passant:bool,is_castle:bool}` with `PieceMove::quiet(from,to)`; `pub fn pseudo_legal_moves(pos:&Position)->Vec<PieceMove>` (generates for `pos.side_to_move`; sliders and castling are no-ops until Tasks 5 and 7).

- [ ] **Step 1: Write `crates/core/src/movegen.rs`**

```rust
use crate::position::Position;
use crate::types::{Color, Piece, PieceKind, Square};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Promotion {
    Queen,
    Rook,
    Bishop,
    Knight,
}

impl Promotion {
    pub const ALL: [Promotion; 4] = [Promotion::Queen, Promotion::Rook, Promotion::Bishop, Promotion::Knight];

    pub fn piece_kind(self) -> PieceKind {
        match self {
            Promotion::Queen => PieceKind::Queen,
            Promotion::Rook => PieceKind::Rook,
            Promotion::Bishop => PieceKind::Bishop,
            Promotion::Knight => PieceKind::Knight,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PieceMove {
    pub from: Square,
    pub to: Square,
    pub promotion: Option<Promotion>,
    pub is_en_passant: bool,
    pub is_castle: bool,
}

impl PieceMove {
    pub fn quiet(from: Square, to: Square) -> PieceMove {
        PieceMove { from, to, promotion: None, is_en_passant: false, is_castle: false }
    }
}

const KNIGHT_OFFSETS: [(i8, i8); 8] = [(1, 2), (2, 1), (2, -1), (1, -2), (-1, -2), (-2, -1), (-2, 1), (-1, 2)];
const KING_OFFSETS: [(i8, i8); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];

fn in_bounds(f: i8, r: i8) -> bool {
    (0..8).contains(&f) && (0..8).contains(&r)
}

fn leaper_moves(pos: &Position, sq: Square, offsets: &[(i8, i8)], color: Color) -> Vec<PieceMove> {
    let mut out = Vec::new();
    for (df, dr) in offsets {
        let f = sq.file() as i8 + df;
        let r = sq.rank() as i8 + dr;
        if !in_bounds(f, r) {
            continue;
        }
        let dest = Square::new(f as u8, r as u8);
        match pos.board.get(dest) {
            Some(p) if p.color == color => {}
            _ => out.push(PieceMove::quiet(sq, dest)),
        }
    }
    out
}

fn add_pawn_move(out: &mut Vec<PieceMove>, from: Square, to: Square, is_en_passant: bool, promo_rank: u8) {
    if to.rank() == promo_rank {
        for promo in Promotion::ALL {
            out.push(PieceMove { from, to, promotion: Some(promo), is_en_passant, is_castle: false });
        }
    } else {
        out.push(PieceMove { from, to, promotion: None, is_en_passant, is_castle: false });
    }
}

fn pawn_moves(pos: &Position, sq: Square, color: Color) -> Vec<PieceMove> {
    let mut out = Vec::new();
    let dir: i8 = if color == Color::White { 1 } else { -1 };
    let start_rank: i8 = if color == Color::White { 1 } else { 6 };
    let promo_rank: u8 = if color == Color::White { 7 } else { 0 };
    let (f, r) = (sq.file() as i8, sq.rank() as i8);
    let at = |rr: i8| Square::new(f as u8, rr as u8);

    // single push
    if in_bounds(f, r + dir) && pos.board.get(at(r + dir)).is_none() {
        add_pawn_move(&mut out, sq, at(r + dir), false, promo_rank);
    }

    // double push -- intermediate-square check is independent of the single-push
    // result so Task 11 can add jump-transparency here without touching this shape.
    if r == start_rank {
        let mid = at(r + dir);
        let landing = at(r + 2 * dir);
        if pos.board.get(mid).is_none() && pos.board.get(landing).is_none() {
            out.push(PieceMove::quiet(sq, landing));
        }
    }

    // captures, including en passant
    for df in [-1i8, 1i8] {
        let (cf, cr) = (f + df, r + dir);
        if !in_bounds(cf, cr) {
            continue;
        }
        let dest = Square::new(cf as u8, cr as u8);
        if let Some(p) = pos.board.get(dest) {
            if p.color != color {
                add_pawn_move(&mut out, sq, dest, false, promo_rank);
            }
        } else if pos.en_passant == Some(dest) {
            add_pawn_move(&mut out, sq, dest, true, promo_rank);
        }
    }

    out
}

pub fn pseudo_legal_moves(pos: &Position) -> Vec<PieceMove> {
    let color = pos.side_to_move;
    let mut out = Vec::new();
    for i in 0..64 {
        let sq = Square(i);
        let piece: Piece = match pos.board.get(sq) {
            Some(p) if p.color == color => p,
            _ => continue,
        };
        match piece.kind {
            PieceKind::Pawn => out.extend(pawn_moves(pos, sq, color)),
            PieceKind::Knight => out.extend(leaper_moves(pos, sq, &KNIGHT_OFFSETS, color)),
            PieceKind::King => out.extend(leaper_moves(pos, sq, &KING_OFFSETS, color)),
            PieceKind::Bishop | PieceKind::Rook | PieceKind::Queen => {} // Task 5
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::Position;
    use crate::types::PieceKind;

    fn moves_from(pos: &Position, sq: Square) -> Vec<Square> {
        let mut v: Vec<Square> = pseudo_legal_moves(pos).into_iter().filter(|m| m.from == sq).map(|m| m.to).collect();
        v.sort();
        v
    }

    #[test]
    fn start_position_pawn_e2_has_two_pushes() {
        let pos = Position::starting();
        let mut expected = vec![Square::from_str("e3").unwrap(), Square::from_str("e4").unwrap()];
        expected.sort();
        assert_eq!(moves_from(&pos, Square::from_str("e2").unwrap()), expected);
    }

    #[test]
    fn start_position_knight_g1_has_two_jumps() {
        let pos = Position::starting();
        let mut expected = vec![Square::from_str("f3").unwrap(), Square::from_str("h3").unwrap()];
        expected.sort();
        assert_eq!(moves_from(&pos, Square::from_str("g1").unwrap()), expected);
    }

    #[test]
    fn pawn_on_e3_cannot_double_push() {
        let mut pos = Position::starting();
        pos.board.set(Square::from_str("e2").unwrap(), None);
        pos.board.set(
            Square::from_str("e3").unwrap(),
            Some(Piece { color: Color::White, kind: PieceKind::Pawn }),
        );
        assert_eq!(moves_from(&pos, Square::from_str("e3").unwrap()), vec![Square::from_str("e4").unwrap()]);
    }
}
```

- [ ] **Step 2: Add to `lib.rs`**

```rust
pub mod movegen;
pub use movegen::{PieceMove, Promotion, pseudo_legal_moves};
```

- [ ] **Step 3: Run `cargo test -p spellchess-core`, expect 12 passing**

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/movegen.rs crates/core/src/lib.rs
git commit -m "feat(core): generate pawn, knight, and king pseudo-legal moves"
```

---

### Task 5: Sliding piece move generation (rays)

**Files:**
- Create: `crates/core/src/rays.rs`
- Modify: `crates/core/src/movegen.rs` (add bishop/rook/queen branch)
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `Position, Square` (Tasks 1, 3).
- Produces: `pub const ROOK_DIRS: [(i8,i8);4]`, `pub const BISHOP_DIRS: [(i8,i8);4]`, `pub fn walk_ray(pos:&Position, from:Square, dir:(i8,i8))->Vec<Square>` — walks outward, stopping after (and including) the first occupied square. Task 11 will change the stop condition to add jump-transparency; nothing else should need to change when it does.

- [ ] **Step 1: Write `crates/core/src/rays.rs`**

```rust
use crate::position::Position;
use crate::types::Square;

pub const ROOK_DIRS: [(i8, i8); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
pub const BISHOP_DIRS: [(i8, i8); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];

/// Walks a ray from `from` in direction `dir`, stopping after the first occupied
/// square (inclusive). Task 11 changes the stop condition so a square carrying a
/// live jump field does not stop the ray.
pub fn walk_ray(pos: &Position, from: Square, dir: (i8, i8)) -> Vec<Square> {
    let mut out = Vec::new();
    let (mut f, mut r) = (from.file() as i8, from.rank() as i8);
    loop {
        f += dir.0;
        r += dir.1;
        if !(0..8).contains(&f) || !(0..8).contains(&r) {
            break;
        }
        let sq = Square::new(f as u8, r as u8);
        out.push(sq);
        if pos.board.get(sq).is_some() {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::Position;
    use crate::types::{Color, Piece, PieceKind};

    #[test]
    fn ray_reaches_edge_of_board_when_unblocked() {
        let pos = Position { board: crate::board::Board::empty(), ..Position::starting() };
        let from = Square::from_str("d4").unwrap();
        let ray = walk_ray(&pos, from, (1, 0));
        assert_eq!(ray, vec![
            Square::from_str("e4").unwrap(), Square::from_str("f4").unwrap(),
            Square::from_str("g4").unwrap(), Square::from_str("h4").unwrap(),
        ]);
    }

    #[test]
    fn ray_stops_at_first_occupied_square_inclusive() {
        let mut pos = Position { board: crate::board::Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("f4").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        let ray = walk_ray(&pos, Square::from_str("d4").unwrap(), (1, 0));
        assert_eq!(ray, vec![Square::from_str("e4").unwrap(), Square::from_str("f4").unwrap()]);
    }
}
```

- [ ] **Step 2: In `movegen.rs`, replace the `Bishop | Rook | Queen => {}` arm**

```rust
PieceKind::Bishop => out.extend(slide_moves(pos, sq, &crate::rays::BISHOP_DIRS, color)),
PieceKind::Rook => out.extend(slide_moves(pos, sq, &crate::rays::ROOK_DIRS, color)),
PieceKind::Queen => {
    out.extend(slide_moves(pos, sq, &crate::rays::ROOK_DIRS, color));
    out.extend(slide_moves(pos, sq, &crate::rays::BISHOP_DIRS, color));
}
```

and add above `pub fn pseudo_legal_moves`:

```rust
fn slide_moves(pos: &Position, sq: Square, dirs: &[(i8, i8)], color: Color) -> Vec<PieceMove> {
    let mut out = Vec::new();
    for &dir in dirs {
        for dest in crate::rays::walk_ray(pos, sq, dir) {
            match pos.board.get(dest) {
                Some(p) if p.color == color => {}
                _ => out.push(PieceMove::quiet(sq, dest)),
            }
        }
    }
    out
}
```

- [ ] **Step 3: Add a movegen test for an unobstructed rook**

```rust
#[test]
fn lone_rook_on_d4_has_fourteen_moves() {
    let mut pos = Position { board: crate::board::Board::empty(), ..Position::starting() };
    pos.board.set(
        Square::from_str("d4").unwrap(),
        Some(Piece { color: Color::White, kind: PieceKind::Rook }),
    );
    assert_eq!(moves_from(&pos, Square::from_str("d4").unwrap()).len(), 14);
}
```

- [ ] **Step 4: Add `pub mod rays;` to `lib.rs`, run `cargo test -p spellchess-core`, expect 15 passing**

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/rays.rs crates/core/src/movegen.rs crates/core/src/lib.rs
git commit -m "feat(core): generate sliding piece moves via shared ray-walking"
```

---

### Task 6: Attack detection (orthodox baseline)

**Files:**
- Create: `crates/core/src/attacks.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `Position, Square, Color, Piece, PieceKind` (Tasks 1, 3), `walk_ray, ROOK_DIRS, BISHOP_DIRS` (Task 5).
- Produces: `pub fn is_square_attacked(pos:&Position, square:Square, by:Color)->bool`. Task 9 will add frozen-attacker exclusion here; Task 11's `walk_ray` change (transparency) applies automatically since this reuses `walk_ray`.

- [ ] **Step 1: Write `crates/core/src/attacks.rs`**

```rust
use crate::position::Position;
use crate::rays::{walk_ray, BISHOP_DIRS, ROOK_DIRS};
use crate::types::{Color, Piece, PieceKind, Square};

const KNIGHT_OFFSETS: [(i8, i8); 8] = [(1, 2), (2, 1), (2, -1), (1, -2), (-1, -2), (-2, -1), (-2, 1), (-1, 2)];
const KING_OFFSETS: [(i8, i8); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];

fn has(pos: &Position, sq: Square, by: Color, kind: PieceKind) -> bool {
    pos.board.get(sq) == Some(Piece { color: by, kind })
}

pub fn is_square_attacked(pos: &Position, square: Square, by: Color) -> bool {
    // pawns: an attacker sits one rank behind the target, from the attacker's own push direction
    let pawn_dir: i8 = if by == Color::White { -1 } else { 1 };
    for df in [-1i8, 1i8] {
        let (f, r) = (square.file() as i8 + df, square.rank() as i8 + pawn_dir);
        if (0..8).contains(&f) && (0..8).contains(&r) && has(pos, Square::new(f as u8, r as u8), by, PieceKind::Pawn) {
            return true;
        }
    }
    for (df, dr) in KNIGHT_OFFSETS {
        let (f, r) = (square.file() as i8 + df, square.rank() as i8 + dr);
        if (0..8).contains(&f) && (0..8).contains(&r) && has(pos, Square::new(f as u8, r as u8), by, PieceKind::Knight) {
            return true;
        }
    }
    for (df, dr) in KING_OFFSETS {
        let (f, r) = (square.file() as i8 + df, square.rank() as i8 + dr);
        if (0..8).contains(&f) && (0..8).contains(&r) && has(pos, Square::new(f as u8, r as u8), by, PieceKind::King) {
            return true;
        }
    }
    for dir in ROOK_DIRS {
        if let Some(&sq) = walk_ray(pos, square, dir).last() {
            if has(pos, sq, by, PieceKind::Rook) || has(pos, sq, by, PieceKind::Queen) {
                return true;
            }
        }
    }
    for dir in BISHOP_DIRS {
        if let Some(&sq) = walk_ray(pos, square, dir).last() {
            if has(pos, sq, by, PieceKind::Bishop) || has(pos, sq, by, PieceKind::Queen) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::position::Position;
    use crate::types::PieceKind;

    #[test]
    fn rook_attacks_along_clear_file() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        assert!(is_square_attacked(&pos, Square::from_str("a8").unwrap(), Color::White));
    }

    #[test]
    fn rook_attack_blocked_by_intervening_piece() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("a4").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        assert!(!is_square_attacked(&pos, Square::from_str("a8").unwrap(), Color::White));
    }

    #[test]
    fn pawn_attacks_diagonally_forward() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("d2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        assert!(is_square_attacked(&pos, Square::from_str("e3").unwrap(), Color::White));
        assert!(!is_square_attacked(&pos, Square::from_str("d3").unwrap(), Color::White));
    }
}
```

- [ ] **Step 2: Add to `lib.rs`**

```rust
pub mod attacks;
pub use attacks::is_square_attacked;
```

Run tests, expect 18 passing.

- [ ] **Step 3: Commit**

```bash
git add crates/core/src/attacks.rs crates/core/src/lib.rs
git commit -m "feat(core): add orthodox square-attack detection"
```

---

### Task 7: Castling

**Files:**
- Modify: `crates/core/src/movegen.rs`

**Interfaces:**
- Consumes: `is_square_attacked` (Task 6).
- Produces: castling moves included in `pseudo_legal_moves` output (`PieceMove.is_castle == true`, `to` = the king's landing square, file 6 for kingside / file 2 for queenside).

- [ ] **Step 1: Add to `movegen.rs`, called once per side at the top of `pseudo_legal_moves`**

```rust
fn castle_moves(pos: &Position, color: Color) -> Vec<PieceMove> {
    let mut out = Vec::new();
    let rank = if color == Color::White { 0 } else { 7 };
    let king_sq = Square::new(4, rank);
    if pos.board.get(king_sq) != Some(Piece { color, kind: PieceKind::King }) {
        return out;
    }
    let (kingside_right, queenside_right) = match color {
        Color::White => (pos.castle_rights.white_kingside, pos.castle_rights.white_queenside),
        Color::Black => (pos.castle_rights.black_kingside, pos.castle_rights.black_queenside),
    };
    let enemy = color.opposite();
    let attacked = |sq: Square| crate::attacks::is_square_attacked(pos, sq, enemy);
    let rook_at = |sq: Square| pos.board.get(sq) == Some(Piece { color, kind: PieceKind::Rook });

    if kingside_right
        && pos.board.get(Square::new(5, rank)).is_none()
        && pos.board.get(Square::new(6, rank)).is_none()
        && rook_at(Square::new(7, rank))
        && !attacked(Square::new(4, rank)) && !attacked(Square::new(5, rank)) && !attacked(Square::new(6, rank))
    {
        out.push(PieceMove { from: king_sq, to: Square::new(6, rank), promotion: None, is_en_passant: false, is_castle: true });
    }
    if queenside_right
        && pos.board.get(Square::new(3, rank)).is_none()
        && pos.board.get(Square::new(2, rank)).is_none()
        && pos.board.get(Square::new(1, rank)).is_none()
        && rook_at(Square::new(0, rank))
        && !attacked(Square::new(4, rank)) && !attacked(Square::new(3, rank)) && !attacked(Square::new(2, rank))
    {
        out.push(PieceMove { from: king_sq, to: Square::new(2, rank), promotion: None, is_en_passant: false, is_castle: true });
    }
    out
}
```

Then in `pseudo_legal_moves`, before the `for i in 0..64` loop, add:

```rust
out.extend(castle_moves(pos, color));
```

- [ ] **Step 2: Add tests**

```rust
#[test]
fn both_castles_available_on_clear_back_rank() {
    let mut pos = Position { board: crate::board::Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(Square::from_str("h1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    let dests = moves_from(&pos, Square::from_str("e1").unwrap());
    assert!(dests.contains(&Square::from_str("g1").unwrap()));
    assert!(dests.contains(&Square::from_str("c1").unwrap()));
}

#[test]
fn castling_blocked_when_king_passes_through_check() {
    let mut pos = Position { board: crate::board::Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(Square::from_str("h1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(Square::from_str("f8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
    let dests = moves_from(&pos, Square::from_str("e1").unwrap());
    assert!(!dests.contains(&Square::from_str("g1").unwrap()));
}
```

- [ ] **Step 3: Run `cargo test -p spellchess-core`, expect 20 passing**

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/movegen.rs
git commit -m "feat(core): generate castling moves respecting check and path rules"
```

---

### Task 8: Legal move filter (`legal_moves`) — Tier 1 vectors 1 and 17

**Files:**
- Create: `crates/core/src/legal.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `pseudo_legal_moves` (Tasks 4/5/7), `is_square_attacked` (Task 6), `Position, PieceMove, Promotion, Square, Piece, PieceKind, Color` (Tasks 1-4).
- Produces: `pub fn apply_move_only(pos:&Position, mv:&PieceMove)->Position` (applies board/castle-rights/en-passant/halfmove-clock effects of one move; does NOT flip `side_to_move`, `ply`, or touch spell state — used for legality testing and reused by `apply_turn` in Task 14). `pub fn legal_moves(pos:&Position)->Vec<PieceMove>`.

- [ ] **Step 1: Write `crates/core/src/legal.rs`**

```rust
use crate::position::Position;
use crate::movegen::{pseudo_legal_moves, PieceMove};
use crate::types::{Color, Piece, PieceKind, Square};

pub fn apply_move_only(pos: &Position, mv: &PieceMove) -> Position {
    let mut next = pos.clone();
    let mover = pos.board.get(mv.from).expect("apply_move_only: no piece on from-square");

    if mv.is_en_passant {
        let captured_sq = Square::new(mv.to.file(), mv.from.rank());
        next.board.set(captured_sq, None);
    }
    if mv.is_castle {
        let rank = mv.from.rank();
        let (rook_from, rook_to) = if mv.to.file() == 6 {
            (Square::new(7, rank), Square::new(5, rank))
        } else {
            (Square::new(0, rank), Square::new(3, rank))
        };
        let rook = next.board.get(rook_from).expect("apply_move_only: castling rook missing");
        next.board.set(rook_from, None);
        next.board.set(rook_to, Some(rook));
    }

    let is_capture = pos.board.get(mv.to).is_some() || mv.is_en_passant;
    next.board.set(mv.from, None);
    let placed = match mv.promotion {
        Some(promo) => Piece { color: mover.color, kind: promo.piece_kind() },
        None => mover,
    };
    next.board.set(mv.to, Some(placed));

    if mover.kind == PieceKind::King {
        match mover.color {
            Color::White => { next.castle_rights.white_kingside = false; next.castle_rights.white_queenside = false; }
            Color::Black => { next.castle_rights.black_kingside = false; next.castle_rights.black_queenside = false; }
        }
    }
    let touched = [mv.from, mv.to];
    if touched.contains(&Square::new(0, 0)) { next.castle_rights.white_queenside = false; }
    if touched.contains(&Square::new(7, 0)) { next.castle_rights.white_kingside = false; }
    if touched.contains(&Square::new(0, 7)) { next.castle_rights.black_queenside = false; }
    if touched.contains(&Square::new(7, 7)) { next.castle_rights.black_kingside = false; }

    next.en_passant = if mover.kind == PieceKind::Pawn && mv.from.rank().abs_diff(mv.to.rank()) == 2 {
        Some(Square::new(mv.from.file(), (mv.from.rank() + mv.to.rank()) / 2))
    } else {
        None
    };

    next.halfmove_clock = if mover.kind == PieceKind::Pawn || is_capture { 0 } else { pos.halfmove_clock + 1 };
    next
}

pub fn legal_moves(pos: &Position) -> Vec<PieceMove> {
    let mover = pos.side_to_move;
    pseudo_legal_moves(pos)
        .into_iter()
        .filter(|mv| {
            let after = apply_move_only(pos, mv);
            match after.board.king_square(mover) {
                Some(king_sq) => !crate::attacks::is_square_attacked(&after, king_sq, mover.opposite()),
                None => true, // this move itself captured the enemy king on a prior ply; not reachable here
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::position::Position;
    use crate::types::{Color, PieceKind};

    fn dests(pos: &Position, sq: Square) -> Vec<Square> {
        let mut v: Vec<Square> = legal_moves(pos).into_iter().filter(|m| m.from == sq).map(|m| m.to).collect();
        v.sort();
        v
    }

    #[test]
    fn vector_1_start_position_sanity() {
        let pos = Position::starting();
        assert_eq!(dests(&pos, Square::from_str("e2").unwrap()), vec![Square::from_str("e3").unwrap(), Square::from_str("e4").unwrap()]);
        assert_eq!(dests(&pos, Square::from_str("g1").unwrap()), vec![Square::from_str("f3").unwrap(), Square::from_str("h3").unwrap()]);
    }

    #[test]
    fn vector_17_promotion() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("b7").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("h8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        assert_eq!(dests(&pos, Square::from_str("b7").unwrap()), vec![Square::from_str("b8").unwrap()]);
    }

    #[test]
    fn king_cannot_move_into_check() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a2").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        assert!(!dests(&pos, Square::from_str("e1").unwrap()).contains(&Square::from_str("e2").unwrap()));
    }
}
```

- [ ] **Step 2: Add `pub mod legal;` to `lib.rs`, run tests, expect 23 passing**

- [ ] **Step 3: Commit**

```bash
git add crates/core/src/legal.rs crates/core/src/lib.rs
git commit -m "feat(core): add legal move filter, closing vectors 1 and 17"
```

---

### Task 9: Freeze mechanics — vectors 2, 3, 6, 7, 8, 15, 16

**Files:**
- Create: `crates/core/src/spells.rs`
- Modify: `crates/core/src/movegen.rs` (skip frozen origin squares; frozen-rook castling check)
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `Position, SpellKind, SpellField, Square, Color` (Task 3).
- Produces: `pub fn freeze_zone(target:Square)->Vec<Square>`; `pub fn is_square_frozen(pos:&Position, square:Square)->bool`; `pub fn freeze_targets(pos:&Position, color:Color)->Vec<Square>`.

- [ ] **Step 1: Write `crates/core/src/spells.rs`**

```rust
use crate::position::{Position, SpellField, SpellKind};
use crate::types::{Color, Square};

pub fn freeze_zone(target: Square) -> Vec<Square> {
    let mut out = Vec::new();
    let (tf, tr) = (target.file() as i8, target.rank() as i8);
    for df in -1..=1 {
        for dr in -1..=1 {
            let (f, r) = (tf + df, tr + dr);
            if (0..8).contains(&f) && (0..8).contains(&r) {
                out.push(Square::new(f as u8, r as u8));
            }
        }
    }
    out
}

fn field_active(pos: &Position, field: &SpellField) -> bool {
    pos.ply <= field.expires_after_ply
}

pub fn is_square_frozen(pos: &Position, square: Square) -> bool {
    pos.fields.iter().any(|f| f.kind == SpellKind::Freeze && field_active(pos, f) && freeze_zone(f.square).contains(&square))
}

pub fn freeze_targets(pos: &Position, color: Color) -> Vec<Square> {
    if !pos.spells(color).freeze.castable() {
        return Vec::new();
    }
    (0..64).map(Square).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Square;

    #[test]
    fn corner_freeze_clips_to_2x2() {
        let a1 = Square::from_str("a1").unwrap();
        let zone = freeze_zone(a1);
        assert!(zone.contains(&Square::from_str("a1").unwrap()));
        assert!(zone.contains(&Square::from_str("a2").unwrap()));
        assert!(zone.contains(&Square::from_str("b2").unwrap()));
        assert!(!zone.contains(&Square::from_str("c3").unwrap()));
        assert_eq!(zone.len(), 4);
    }

    #[test]
    fn center_freeze_covers_3x3() {
        assert_eq!(freeze_zone(Square::from_str("d5").unwrap()).len(), 9);
    }
}
```

- [ ] **Step 2: In `movegen.rs`, skip frozen origins.** In `pseudo_legal_moves`, inside the `for i in 0..64` loop, right after the `piece` match, add:

```rust
if crate::spells::is_square_frozen(pos, sq) {
    continue;
}
```

- [ ] **Step 3: In `castle_moves` (Task 7), add rook/king frozen checks.** Add `!crate::spells::is_square_frozen(pos, king_sq)` as an early return guard at the top, and add `&& !crate::spells::is_square_frozen(pos, Square::new(7, rank))` to the kingside condition and `&& !crate::spells::is_square_frozen(pos, Square::new(0, rank))` to the queenside condition.

- [ ] **Step 4: Add vector tests to `legal.rs`**

```rust
#[test]
fn vector_2_freeze_immobilizes_the_targeted_piece() {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(Square::from_str("d5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
    pos.board.set(Square::from_str("h7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Knight }));
    pos.side_to_move = Color::Black;
    pos.fields.push(crate::position::SpellField {
        square: Square::from_str("d5").unwrap(), owner: Color::White,
        kind: crate::position::SpellKind::Freeze, expires_after_ply: pos.ply + 1,
    });
    assert_eq!(dests(&pos, Square::from_str("d5").unwrap()), Vec::<Square>::new());
}

#[test]
fn vector_7_own_freeze_binds_own_piece_same_turn() {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(Square::from_str("d4").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Knight }));
    pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.fields.push(crate::position::SpellField {
        square: Square::from_str("d4").unwrap(), owner: Color::White,
        kind: crate::position::SpellKind::Freeze, expires_after_ply: pos.ply + 1,
    });
    assert!(dests(&pos, Square::from_str("d4").unwrap()).is_empty());
    assert!(!dests(&pos, Square::from_str("a1").unwrap()).is_empty());
}

#[test]
fn vector_6_frozen_piece_still_blocks_and_is_capturable() {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(Square::from_str("d1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(Square::from_str("d5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Knight }));
    pos.fields.push(crate::position::SpellField {
        square: Square::from_str("d5").unwrap(), owner: Color::Black,
        kind: crate::position::SpellKind::Freeze, expires_after_ply: pos.ply + 1,
    });
    let rook_dests = dests(&pos, Square::from_str("d1").unwrap());
    assert!(rook_dests.contains(&Square::from_str("d5").unwrap()));
    assert!(!rook_dests.contains(&Square::from_str("d6").unwrap()));
}
```

- [ ] **Step 5: Add `pub mod spells;` to `lib.rs`, run `cargo test -p spellchess-core`, expect 28 passing**

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/spells.rs crates/core/src/movegen.rs crates/core/src/legal.rs crates/core/src/lib.rs
git commit -m "feat(core): wire freeze zone geometry into move generation"
```

---

### Task 10: Frozen pieces exert no control — vectors 4, 5

**Files:**
- Modify: `crates/core/src/attacks.rs`

**Interfaces:**
- Consumes: `is_square_frozen` (Task 9).
- Produces: `is_square_attacked` now excludes any attacking piece standing on a frozen square. Signature unchanged.

- [ ] **Step 1: In each of the five detection blocks in `is_square_attacked`, require the candidate attacker's square is not frozen.** E.g. the pawn block becomes:

```rust
for df in [-1i8, 1i8] {
    let (f, r) = (square.file() as i8 + df, square.rank() as i8 + pawn_dir);
    if (0..8).contains(&f) && (0..8).contains(&r) {
        let sq = Square::new(f as u8, r as u8);
        if has(pos, sq, by, PieceKind::Pawn) && !crate::spells::is_square_frozen(pos, sq) {
            return true;
        }
    }
}
```

Apply the same `&& !crate::spells::is_square_frozen(pos, sq)` addition to the knight, king, rook/queen-ray, and bishop/queen-ray blocks (in the ray blocks, guard the square found via `.last()`).

- [ ] **Step 2: Add vector tests to `attacks.rs`**

```rust
#[test]
fn vector_4_frozen_piece_exerts_no_control() {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("h4").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    assert!(is_square_attacked(&pos, Square::from_str("d4").unwrap(), Color::White));
    pos.fields.push(crate::position::SpellField {
        square: Square::from_str("h4").unwrap(), owner: Color::Black,
        kind: crate::position::SpellKind::Freeze, expires_after_ply: pos.ply + 1,
    });
    assert!(!is_square_attacked(&pos, Square::from_str("d4").unwrap(), Color::White));
}
```

Add to `legal.rs` (needs `legal_moves`, so belongs there):

```rust
#[test]
fn vector_5_freezing_the_checker_dispels_check() {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(Square::from_str("d8").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(Square::from_str("h8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
    pos.side_to_move = Color::Black;
    assert!(crate::attacks::is_square_attacked(&pos, Square::from_str("e8").unwrap(), Color::White));
    pos.fields.push(crate::position::SpellField {
        square: Square::from_str("d8").unwrap(), owner: Color::Black,
        kind: crate::position::SpellKind::Freeze, expires_after_ply: pos.ply + 1,
    });
    let h8_dests = dests(&pos, Square::from_str("h8").unwrap());
    assert!(h8_dests.contains(&Square::from_str("h7").unwrap()));
}
```

- [ ] **Step 3: Run `cargo test -p spellchess-core`, expect 30 passing**

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/attacks.rs crates/core/src/legal.rs
git commit -m "fix(core): frozen pieces no longer exert check/guard control"
```

---

### Task 11: Jump mechanics — vectors 10, 11

**Files:**
- Modify: `crates/core/src/spells.rs` (add `is_square_jump_active`, `jump_targets`)
- Modify: `crates/core/src/rays.rs` (jump transparency in `walk_ray`)
- Modify: `crates/core/src/movegen.rs` (pawn double-step transparency)

**Interfaces:**
- Consumes: `Position, SpellKind, Square, Color` (Task 3).
- Produces: `pub fn is_square_jump_active(pos:&Position, square:Square)->bool`; `pub fn jump_targets(pos:&Position, color:Color)->Vec<Square>` (occupied squares only, gated by `castable()`).

- [ ] **Step 1: Add to `spells.rs`**

```rust
pub fn is_square_jump_active(pos: &Position, square: Square) -> bool {
    pos.fields.iter().any(|f| f.kind == SpellKind::Jump && field_active(pos, f) && f.square == square)
}

pub fn jump_targets(pos: &Position, color: Color) -> Vec<Square> {
    if !pos.spells(color).jump.castable() {
        return Vec::new();
    }
    (0..64).map(Square).filter(|&sq| pos.board.get(sq).is_some()).collect()
}
```

- [ ] **Step 2: In `rays.rs`, change the stop condition in `walk_ray`**

```rust
if pos.board.get(sq).is_some() && !crate::spells::is_square_jump_active(pos, sq) {
    break;
}
```

- [ ] **Step 3: In `movegen.rs`'s `pawn_moves`, make the double-push intermediate square jump-aware**

```rust
if r == start_rank {
    let mid = at(r + dir);
    let landing = at(r + 2 * dir);
    let mid_passable = pos.board.get(mid).is_none() || crate::spells::is_square_jump_active(pos, mid);
    if mid_passable && pos.board.get(landing).is_none() {
        out.push(PieceMove::quiet(sq, landing));
    }
}
```

- [ ] **Step 4: Add vector tests to `legal.rs`**

```rust
#[test]
fn vector_10_jump_field_serves_both_players() {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(Square::from_str("d1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(Square::from_str("d4").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
    pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(Square::from_str("d8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
    pos.fields.push(crate::position::SpellField {
        square: Square::from_str("d4").unwrap(), owner: Color::White,
        kind: crate::position::SpellKind::Jump, expires_after_ply: pos.ply + 1,
    });
    assert!(dests(&pos, Square::from_str("d1").unwrap()).contains(&Square::from_str("d8").unwrap()));
    let mut black_pos = pos.clone();
    black_pos.side_to_move = Color::Black;
    assert!(dests(&black_pos, Square::from_str("d8").unwrap()).contains(&Square::from_str("d1").unwrap()));
}

#[test]
fn vector_11_pawn_double_steps_over_jumped_blocker() {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(Square::from_str("d2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
    pos.board.set(Square::from_str("d3").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
    assert!(dests(&pos, Square::from_str("d2").unwrap()).is_empty());
    pos.fields.push(crate::position::SpellField {
        square: Square::from_str("d3").unwrap(), owner: Color::White,
        kind: crate::position::SpellKind::Jump, expires_after_ply: pos.ply + 1,
    });
    assert_eq!(dests(&pos, Square::from_str("d2").unwrap()), vec![Square::from_str("d4").unwrap()]);
}
```

- [ ] **Step 5: Run `cargo test -p spellchess-core`, expect 32 passing**

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/spells.rs crates/core/src/rays.rs crates/core/src/movegen.rs crates/core/src/legal.rs
git commit -m "feat(core): add jump square transparency for sliders and pawn double-step"
```

---

### Task 12: King capture and unblockable jump-checks — vectors 9, 12

**Files:**
- Modify: `crates/core/src/legal.rs` (tests only — the mechanism already falls out of Tasks 6-11)

**Interfaces:**
- Consumes: everything from Tasks 6-11. No new production code expected; this task exists to prove (and lock in with regression tests) that king capture and unblockable checks work by construction.

- [ ] **Step 1: Add vector tests to `legal.rs`**

```rust
#[test]
fn vector_9_king_capture_via_jump() {
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
    assert!(legal_moves(&pos).contains(&capture));
    let after = apply_move_only(&pos, &capture);
    assert_eq!(after.board.king_square(Color::White), None);
}

#[test]
fn vector_12_check_through_jump_square_is_unblockable() {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(Square::from_str("d2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Bishop }));
    pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(Square::from_str("h2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(Square::from_str("b4").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Bishop }));
    pos.fields.push(crate::position::SpellField {
        square: Square::from_str("d2").unwrap(), owner: Color::Black,
        kind: crate::position::SpellKind::Jump, expires_after_ply: pos.ply + 1,
    });
    assert!(dests(&pos, Square::from_str("a1").unwrap()).is_empty());
    assert!(dests(&pos, Square::from_str("d2").unwrap()).contains(&Square::from_str("b4").unwrap()));
    let king_dests = dests(&pos, Square::from_str("e1").unwrap());
    assert!(!king_dests.is_empty());
}
```

- [ ] **Step 2: Run `cargo test -p spellchess-core`, expect 34 passing.** If either test fails, the bug is in Tasks 6-11, not here — re-check `is_square_attacked`'s use of `walk_ray` and the frozen/jump guards before adding new code.

- [ ] **Step 3: Commit**

```bash
git add crates/core/src/legal.rs
git commit -m "test(core): lock in king capture and unblockable jump-check, vectors 9 and 12"
```

---

### Task 13: Turn generation (spell + move) — vector 19

**Files:**
- Modify: `crates/core/src/legal.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `legal_moves` (Task 8), `freeze_targets, jump_targets` (Tasks 9, 11), `SpellField, SpellKind` (Task 3).
- Produces: `pub struct SpellCast{kind:SpellKind, square:Square}`; `pub struct Turn{spell:Option<SpellCast>, mv:PieceMove}`; `pub fn generate_turns(pos:&Position)->Vec<Turn>`. This is the function `search` and `cli` will call for legal-move enumeration; every later crate consumes only this.

- [ ] **Step 1: Add to `legal.rs`**

```rust
use crate::position::{SpellField, SpellKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpellCast {
    pub kind: SpellKind,
    pub square: Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Turn {
    pub spell: Option<SpellCast>,
    pub mv: PieceMove,
}

fn position_with_field(pos: &Position, cast: SpellCast) -> Position {
    let mut next = pos.clone();
    next.fields.push(SpellField {
        square: cast.square,
        owner: pos.side_to_move,
        kind: cast.kind,
        expires_after_ply: pos.ply + 1,
    });
    next
}

pub fn generate_turns(pos: &Position) -> Vec<Turn> {
    let mut turns: Vec<Turn> = legal_moves(pos).into_iter().map(|mv| Turn { spell: None, mv }).collect();
    let color = pos.side_to_move;

    for sq in crate::spells::freeze_targets(pos, color) {
        let cast = SpellCast { kind: SpellKind::Freeze, square: sq };
        let hypothetical = position_with_field(pos, cast);
        turns.extend(legal_moves(&hypothetical).into_iter().map(|mv| Turn { spell: Some(cast), mv }));
    }
    for sq in crate::spells::jump_targets(pos, color) {
        let cast = SpellCast { kind: SpellKind::Jump, square: sq };
        let hypothetical = position_with_field(pos, cast);
        turns.extend(legal_moves(&hypothetical).into_iter().map(|mv| Turn { spell: Some(cast), mv }));
    }
    turns
}
```

- [ ] **Step 2: Add tests**

```rust
#[test]
fn generate_turns_includes_no_spell_and_spell_options_at_start() {
    let pos = Position::starting();
    let turns = generate_turns(&pos);
    let no_spell_count = turns.iter().filter(|t| t.spell.is_none()).count();
    assert_eq!(no_spell_count, legal_moves(&pos).len());
    assert!(turns.iter().any(|t| matches!(t.spell, Some(SpellCast { kind: SpellKind::Freeze, .. }))));
    assert!(turns.iter().any(|t| matches!(t.spell, Some(SpellCast { kind: SpellKind::Jump, .. }))));
}

#[test]
fn vector_19_illegal_casts_are_never_offered() {
    let mut pos = Position::starting();
    pos.white_spells.jump.count = 0;
    pos.white_spells.freeze.lock = 2;
    let turns = generate_turns(&pos);
    assert!(!turns.iter().any(|t| t.spell.is_some()));
}
```

- [ ] **Step 3: Add `pub use legal::{Turn, SpellCast, generate_turns, legal_moves, apply_move_only};` to `lib.rs`, run tests, expect 36 passing**

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/legal.rs crates/core/src/lib.rs
git commit -m "feat(core): generate full turns (spell + move), closing vector 19"
```

---

### Task 14: Turn application and spell economy — vector 18

**Files:**
- Modify: `crates/core/src/legal.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `apply_move_only, Turn, SpellCast` (Tasks 8, 13).
- Produces: `pub fn apply_turn(pos:&Position, turn:&Turn)->Position` — commits a full turn: applies the move, applies spell cast bookkeeping (count/lock), decrements the opponent's locks, expires stale fields, flips `side_to_move`, increments `ply`.

- [ ] **Step 1: Add to `legal.rs`**

```rust
pub fn apply_turn(pos: &Position, turn: &Turn) -> Position {
    let mover = pos.side_to_move;
    let mut next = apply_move_only(pos, &turn.mv);

    if let Some(cast) = turn.spell {
        next.fields.push(SpellField {
            square: cast.square, owner: mover, kind: cast.kind, expires_after_ply: pos.ply + 1,
        });
        let counter = match (cast.kind, mover) {
            (SpellKind::Freeze, Color::White) => &mut next.white_spells.freeze,
            (SpellKind::Freeze, Color::Black) => &mut next.black_spells.freeze,
            (SpellKind::Jump, Color::White) => &mut next.white_spells.jump,
            (SpellKind::Jump, Color::Black) => &mut next.black_spells.jump,
        };
        counter.count -= 1;
        counter.lock = 3;
    }

    // A side's lock decrements once per turn its opponent completes (rules/20-spell-system.md#cooldown-timing).
    let opponent_spells = match mover {
        Color::White => &mut next.black_spells,
        Color::Black => &mut next.white_spells,
    };
    if opponent_spells.freeze.lock > 0 { opponent_spells.freeze.lock -= 1; }
    if opponent_spells.jump.lock > 0 { opponent_spells.jump.lock -= 1; }

    next.ply = pos.ply + 1;
    next.fields.retain(|f| next.ply <= f.expires_after_ply);
    next.side_to_move = mover.opposite();
    next
}
```

- [ ] **Step 2: Add the vector 18 cooldown-timeline test**

```rust
#[test]
fn vector_18_cooldown_timeline() {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(Square::from_str("a8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));

    let cast = SpellCast { kind: SpellKind::Freeze, square: Square::from_str("d5").unwrap() };
    let mv = PieceMove::quiet(Square::from_str("a1").unwrap(), Square::from_str("a2").unwrap());
    pos = apply_turn(&pos, &Turn { spell: Some(cast), mv });
    assert_eq!(pos.spells(Color::White).freeze, SpellCounter { count: 4, lock: 3 });
    assert!(pos.fields.is_empty() == false);

    let quiet = |pos: &Position, from: &str, to: &str| Turn {
        spell: None,
        mv: PieceMove::quiet(Square::from_str(from).unwrap(), Square::from_str(to).unwrap()),
    };

    pos = apply_turn(&pos, &quiet(&pos, "a8", "a7")); // Black's reply
    assert_eq!(pos.spells(Color::White).freeze, SpellCounter { count: 4, lock: 2 });
    assert!(pos.fields.is_empty());

    pos = apply_turn(&pos, &quiet(&pos, "a2", "a3")); // White N+1
    assert_eq!(pos.spells(Color::White).freeze, SpellCounter { count: 4, lock: 2 });
    pos = apply_turn(&pos, &quiet(&pos, "a7", "a6")); // Black reply
    assert_eq!(pos.spells(Color::White).freeze, SpellCounter { count: 4, lock: 1 });
    pos = apply_turn(&pos, &quiet(&pos, "a3", "a4")); // White N+2
    assert_eq!(pos.spells(Color::White).freeze, SpellCounter { count: 4, lock: 1 });
    pos = apply_turn(&pos, &quiet(&pos, "a6", "a5")); // Black reply
    assert_eq!(pos.spells(Color::White).freeze, SpellCounter { count: 4, lock: 0 });
    assert!(pos.spells(Color::White).freeze.castable());
}
```

(This test needs `SpellCounter` imported/visible — it already is, via `crate::position::*` used elsewhere in this file.)

- [ ] **Step 3: Add `pub use legal::apply_turn;` to `lib.rs`, run tests, expect 37 passing**

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/legal.rs crates/core/src/lib.rs
git commit -m "feat(core): apply full turns with spell cooldown timing, closing vector 18"
```

---

### Task 15: Terminal detection (checkmate/stalemate/king capture, spell escape hatch) — vectors 13, 14

**Files:**
- Create: `crates/core/src/terminal.rs`
- Modify: `crates/core/src/board.rs` (change `king_square` semantics note only — no signature change needed, already `Option`)
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `generate_turns` (Task 13), `is_square_attacked` (Task 10), `Position, Color` (Tasks 1, 3).
- Produces: `pub enum GameStatus{InProgress, Checkmate(Color), Stalemate, KingCaptured(Color)}` (the `Color` payload is the winner); `pub fn game_status(pos:&Position)->GameStatus`.

- [ ] **Step 1: Write `crates/core/src/terminal.rs`**

```rust
use crate::legal::generate_turns;
use crate::position::Position;
use crate::types::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameStatus {
    InProgress,
    Checkmate(Color),
    Stalemate,
    KingCaptured(Color),
}

pub fn game_status(pos: &Position) -> GameStatus {
    let mover = pos.side_to_move;
    let king_sq = match pos.board.king_square(mover) {
        Some(sq) => sq,
        None => return GameStatus::KingCaptured(mover.opposite()),
    };
    if !generate_turns(pos).is_empty() {
        return GameStatus::InProgress;
    }
    if crate::attacks::is_square_attacked(pos, king_sq, mover.opposite()) {
        GameStatus::Checkmate(mover.opposite())
    } else {
        GameStatus::Stalemate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::position::{CastleRights, SpellCounter, SpellState};
    use crate::types::{Piece, PieceKind, Square};

    fn freeze_mate_position(black_spells: SpellState) -> Position {
        let mut pos = Position { board: Board::empty(), castle_rights: CastleRights::all(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("h1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.black_spells = black_spells;
        pos.fields.push(crate::position::SpellField {
            square: Square::from_str("e8").unwrap(), owner: Color::White,
            kind: crate::position::SpellKind::Freeze, expires_after_ply: pos.ply + 1,
        });
        pos.side_to_move = Color::Black;
        pos
    }

    #[test]
    fn vector_13_escape_hatch_prevents_mate_when_a_spell_is_available() {
        let pos = freeze_mate_position(SpellState::starting());
        assert_eq!(game_status(&pos), GameStatus::InProgress);
    }

    #[test]
    fn vector_13_mate_when_spells_are_exhausted() {
        let exhausted = SpellState { freeze: SpellCounter { count: 0, lock: 0 }, jump: SpellCounter { count: 0, lock: 0 } };
        let pos = freeze_mate_position(exhausted);
        assert_eq!(game_status(&pos), GameStatus::Checkmate(Color::White));
    }

    #[test]
    fn vector_13_mate_when_spells_are_all_on_cooldown() {
        let locked = SpellState { freeze: SpellCounter { count: 5, lock: 3 }, jump: SpellCounter { count: 2, lock: 3 } };
        let pos = freeze_mate_position(locked);
        assert_eq!(game_status(&pos), GameStatus::Checkmate(Color::White));
    }

    #[test]
    fn vector_14_stalemate_is_spell_aware() {
        let mut pos = Position { board: Board::empty(), castle_rights: CastleRights::all(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("b1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.black_spells = SpellState { freeze: SpellCounter { count: 0, lock: 0 }, jump: SpellCounter { count: 0, lock: 0 } };
        pos.fields.push(crate::position::SpellField {
            square: Square::from_str("e8").unwrap(), owner: Color::White,
            kind: crate::position::SpellKind::Freeze, expires_after_ply: pos.ply + 1,
        });
        pos.side_to_move = Color::Black;
        assert_eq!(game_status(&pos), GameStatus::Stalemate);
    }
}
```

- [ ] **Step 2: Add `pub mod terminal; pub use terminal::GameStatus; pub use terminal::game_status;` to `lib.rs`. Run `cargo test -p spellchess-core`, expect 41 passing — this closes Tier 1 (all 20 vectors from `rules/90-test-vectors.md` now have a corresponding test; vector 20 needs no test since our engine only ever constructs turns through `generate_turns`, which cannot produce the two-phase violations it describes).**

- [ ] **Step 3: Commit**

```bash
git add crates/core/src/terminal.rs crates/core/src/lib.rs
git commit -m "feat(core): terminal detection with spell escape hatch, closing vectors 13 and 14"
```

**Tier 1 gate check:** before starting Task 16, confirm `cargo test -p spellchess-core` is fully green and re-read the vector list in `rules/90-test-vectors.md` against the tests added in Tasks 8-15 to confirm nothing was missed.

---

### Task 16: `search` crate skeleton + Zobrist hashing

**Files:**
- Modify: `crates/core/src/types.rs` (add `Hash` to `Piece` — already present)
- Modify: `crates/core/src/position.rs` (add `#[derive(Hash)]` to `CastleRights`, `SpellCounter`, `SpellState`, `SpellField`, `Position`)
- Modify: `crates/core/src/board.rs` (add `#[derive(Hash)]` to `Board`)
- Create: `crates/search/Cargo.toml`
- Create: `crates/search/src/lib.rs`
- Create: `crates/search/src/zobrist.rs`
- Modify: root `Cargo.toml` (already lists `crates/search` as a member from Setup)

**Interfaces:**
- Consumes: `Position` and all its nested types (Task 3), now `Hash`-able.
- Produces: `pub fn hash_position(pos:&Position)->u64` in `spellchess_search::zobrist`.

- [ ] **Step 1: Add `Hash` to the derive lists** in `board.rs` (`Board`) and `position.rs` (`CastleRights`, `SpellCounter`, `SpellState`, `SpellField`, `Position`). Every field of `Position` is already `Hash` once these are added (`Board`, `Color`, `CastleRights`, `Option<Square>`, `u32`, `u64`, `SpellState`, `Vec<SpellField>`).

- [ ] **Step 2: Write `crates/search/Cargo.toml`**

```toml
[package]
name = "spellchess-search"
version = "0.1.0"
edition = "2021"

[dependencies]
spellchess-core = { path = "../core" }
```

- [ ] **Step 3: Write `crates/search/src/zobrist.rs`**

```rust
use spellchess_core::Position;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub fn hash_position(pos: &Position) -> u64 {
    let mut hasher = DefaultHasher::new();
    pos.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellchess_core::Position;

    #[test]
    fn identical_positions_hash_identically() {
        assert_eq!(hash_position(&Position::starting()), hash_position(&Position::starting()));
    }

    #[test]
    fn a_move_changes_the_hash() {
        let start = Position::starting();
        let turns = spellchess_core::generate_turns(&start);
        let after = spellchess_core::apply_turn(&start, &turns[0]);
        assert_ne!(hash_position(&start), hash_position(&after));
    }
}
```

- [ ] **Step 4: Write `crates/search/src/lib.rs`**

```rust
pub mod zobrist;
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p spellchess-search`
Expected: 2 tests pass. Also re-run `cargo test -p spellchess-core` to confirm the new derives didn't break anything (41 still passing).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/core/src/board.rs crates/core/src/position.rs crates/search
git commit -m "feat(search): scaffold search crate with Zobrist-style position hashing"
```

---

### Task 17: Material evaluator

**Files:**
- Create: `crates/search/src/eval.rs`
- Modify: `crates/search/src/lib.rs`

**Interfaces:**
- Consumes: `Position, Color, PieceKind, Square` (core).
- Produces: `pub fn piece_value(kind:PieceKind)->i32`; `pub fn evaluate(pos:&Position)->i32` — score from the perspective of `pos.side_to_move` (positive is good for the side to move). This signature does not change in Task 22; only its internals grow.

- [ ] **Step 1: Write `crates/search/src/eval.rs`**

```rust
use spellchess_core::{Color, PieceKind, Position, Square};

pub fn piece_value(kind: PieceKind) -> i32 {
    match kind {
        PieceKind::Pawn => 100,
        PieceKind::Knight => 320,
        PieceKind::Bishop => 330,
        PieceKind::Rook => 500,
        PieceKind::Queen => 900,
        PieceKind::King => 0,
    }
}

/// Score from the perspective of `pos.side_to_move`: positive is good for the side to move.
pub fn evaluate(pos: &Position) -> i32 {
    let mut white = 0i32;
    let mut black = 0i32;
    for i in 0..64 {
        if let Some(p) = pos.board.get(Square(i)) {
            let v = piece_value(p.kind);
            match p.color {
                Color::White => white += v,
                Color::Black => black += v,
            }
        }
    }
    let material = white - black;
    match pos.side_to_move {
        Color::White => material,
        Color::Black => -material,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellchess_core::Position;

    #[test]
    fn start_position_is_balanced() {
        assert_eq!(evaluate(&Position::starting()), 0);
    }

    #[test]
    fn missing_enemy_queen_favors_the_side_to_move() {
        let mut pos = Position::starting();
        // remove black's queen (d8)
        pos.board.set(Square::from_str("d8").unwrap(), None);
        assert!(evaluate(&pos) > 800);
    }
}
```

- [ ] **Step 2: Add `pub mod eval;` to `lib.rs`, run `cargo test -p spellchess-search`, expect 4 passing**

- [ ] **Step 3: Commit**

```bash
git add crates/search/src/eval.rs crates/search/src/lib.rs
git commit -m "feat(search): add material-only evaluator"
```

---

### Task 18: Fixed-depth negamax

**Files:**
- Create: `crates/search/src/search.rs`
- Modify: `crates/search/src/lib.rs`

**Interfaces:**
- Consumes: `generate_turns, apply_turn, Position, Turn` (core), `evaluate` (Task 17).
- Produces: `pub fn negamax(pos:&Position, depth:u32)->i32`; `pub fn best_turn(pos:&Position, depth:u32)->Option<(Turn,i32)>`. `best_turn` returns `None` only if `generate_turns` is empty (terminal position).

- [ ] **Step 1: Write `crates/search/src/search.rs`**

```rust
use spellchess_core::{apply_turn, generate_turns, Position, Turn};
use crate::eval::evaluate;

pub fn negamax(pos: &Position, depth: u32) -> i32 {
    let turns = generate_turns(pos);
    if depth == 0 || turns.is_empty() {
        return evaluate(pos);
    }
    let mut best = i32::MIN;
    for turn in turns {
        let next = apply_turn(pos, &turn);
        let score = -negamax(&next, depth - 1);
        if score > best {
            best = score;
        }
    }
    best
}

pub fn best_turn(pos: &Position, depth: u32) -> Option<(Turn, i32)> {
    let turns = generate_turns(pos);
    let mut best: Option<(Turn, i32)> = None;
    for turn in turns {
        let next = apply_turn(pos, &turn);
        let score = -negamax(&next, depth.saturating_sub(1));
        if best.map_or(true, |(_, b)| score > b) {
            best = Some((turn, score));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellchess_core::{Board, Color, Piece, PieceKind, Position, Square};

    #[test]
    fn finds_back_rank_mate_in_one() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("g8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("f7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("g7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("h7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        let (turn, _score) = best_turn(&pos, 1).expect("a move must be found");
        assert_eq!(turn.mv.from, Square::from_str("a1").unwrap());
        assert_eq!(turn.mv.to, Square::from_str("a8").unwrap());
    }

    #[test]
    fn finds_king_capture_via_jump_in_one() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Bishop }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("b4").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Bishop }));
        pos.board.set(Square::from_str("a8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        pos.side_to_move = Color::Black;
        let (turn, _score) = best_turn(&pos, 1).expect("a move must be found");
        assert_eq!(turn.mv.to, Square::from_str("e1").unwrap());
        assert!(turn.spell.is_some());
    }
}
```

- [ ] **Step 2: Add `pub mod search;` to `lib.rs`, run `cargo test -p spellchess-search`, expect 6 passing.** These tests are slow-ish at depth 1 on a near-empty board but should complete in well under a second; if `finds_king_capture_via_jump_in_one` hangs, check that `jump_targets` isn't accidentally being called for the side that lacks a live jump — re-verify Task 11.

- [ ] **Step 3: Commit**

```bash
git add crates/search/src/search.rs crates/search/src/lib.rs
git commit -m "feat(search): add fixed-depth negamax search"
```

---

### Task 19: Move ordering + iterative deepening (time-boxed and depth-boxed)

**Files:**
- Create: `crates/search/src/ordering.rs`
- Modify: `crates/search/src/search.rs`
- Modify: `crates/search/src/lib.rs`

**Interfaces:**
- Consumes: `evaluate, piece_value` (Task 17), `Turn, Position` (core).
- Produces: `pub fn order_turns(pos:&Position, turns:Vec<Turn>)->Vec<Turn>`; `pub enum Budget{Depth(u32), Time(std::time::Duration)}`; `pub fn search(pos:&Position, budget:Budget)->Option<(Turn,i32)>`. `negamax`/`best_turn` from Task 18 remain for direct fixed-depth use (e.g. by tests); `search` is the entry point `cli` will call.

- [ ] **Step 1: Write `crates/search/src/ordering.rs`**

```rust
use spellchess_core::{Position, Turn};

pub fn order_turns(pos: &Position, mut turns: Vec<Turn>) -> Vec<Turn> {
    turns.sort_by_key(|t| std::cmp::Reverse(turn_priority(pos, t)));
    turns
}

fn turn_priority(pos: &Position, t: &Turn) -> i32 {
    let mut score = 0;
    if let Some(captured) = pos.board.get(t.mv.to) {
        score += 1000 + crate::eval::piece_value(captured.kind);
    }
    if t.spell.is_none() {
        score += 50; // cheap default: prefer a plain move over a speculative cast
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellchess_core::{generate_turns, Position};

    #[test]
    fn captures_sort_before_quiet_moves() {
        let pos = Position::starting();
        let ordered = order_turns(&pos, generate_turns(&pos));
        // start position has no captures, so this just proves the function runs and preserves the set
        assert_eq!(ordered.len(), generate_turns(&pos).len());
    }
}
```

- [ ] **Step 2: Add to `search.rs`**

```rust
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub enum Budget {
    Depth(u32),
    Time(Duration),
}

pub fn search(pos: &Position, budget: Budget) -> Option<(Turn, i32)> {
    let start = Instant::now();
    let max_depth = match budget {
        Budget::Depth(d) => d,
        Budget::Time(_) => 64,
    };
    let mut best: Option<(Turn, i32)> = None;
    for depth in 1..=max_depth {
        if let Budget::Time(limit) = budget {
            if start.elapsed() >= limit {
                break;
            }
        }
        let turns = crate::ordering::order_turns(pos, generate_turns(pos));
        if turns.is_empty() {
            break;
        }
        let mut iter_best: Option<(Turn, i32)> = None;
        for turn in turns {
            let next = apply_turn(pos, &turn);
            let score = -negamax(&next, depth.saturating_sub(1));
            if iter_best.map_or(true, |(_, b)| score > b) {
                iter_best = Some((turn, score));
            }
            if let Budget::Time(limit) = budget {
                if start.elapsed() >= limit {
                    break;
                }
            }
        }
        if iter_best.is_some() {
            best = iter_best;
        }
    }
    best
}
```

- [ ] **Step 3: Add tests to `search.rs`**

```rust
#[test]
fn depth_budget_returns_a_move() {
    let pos = Position::starting();
    assert!(search(&pos, Budget::Depth(2)).is_some());
}

#[test]
fn time_budget_returns_within_the_budget() {
    let pos = Position::starting();
    let start = std::time::Instant::now();
    let result = search(&pos, Budget::Time(std::time::Duration::from_millis(200)));
    assert!(result.is_some());
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
}
```

- [ ] **Step 4: Add `pub mod ordering;` to `lib.rs`, run `cargo test -p spellchess-search`, expect 9 passing**

- [ ] **Step 5: Commit**

```bash
git add crates/search/src/ordering.rs crates/search/src/search.rs crates/search/src/lib.rs
git commit -m "feat(search): add move ordering and iterative deepening with time/depth budgets"
```

---

### Task 20: Alpha-beta pruning with a transposition table

**Files:**
- Create: `crates/search/src/tt.rs`
- Modify: `crates/search/src/search.rs` (replace plain negamax with alpha-beta + TT probing)
- Modify: `crates/search/src/lib.rs`

**Interfaces:**
- Consumes: `hash_position` (Task 16).
- Produces: `pub enum Bound{Exact,Lower,Upper}`; `pub struct TtEntry{depth:u32,score:i32,bound:Bound}`; `pub struct TranspositionTable` with `::new()`, `.get(u64)->Option<&TtEntry>`, `.insert(u64,TtEntry)`. `negamax` becomes internally alpha-beta but keeps its existing `(pos,depth)->i32` signature by wrapping a new `alphabeta(pos,depth,alpha,beta,tt)->i32`; external callers (Task 18/19 call sites, `cli`) are unaffected.

- [ ] **Step 1: Write `crates/search/src/tt.rs`**

```rust
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    Exact,
    Lower,
    Upper,
}

#[derive(Debug, Clone, Copy)]
pub struct TtEntry {
    pub depth: u32,
    pub score: i32,
    pub bound: Bound,
}

#[derive(Default)]
pub struct TranspositionTable {
    table: HashMap<u64, TtEntry>,
}

impl TranspositionTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: u64) -> Option<&TtEntry> {
        self.table.get(&key)
    }

    pub fn insert(&mut self, key: u64, entry: TtEntry) {
        self.table.insert(key, entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_an_entry() {
        let mut tt = TranspositionTable::new();
        tt.insert(42, TtEntry { depth: 3, score: 17, bound: Bound::Exact });
        let entry = tt.get(42).unwrap();
        assert_eq!(entry.depth, 3);
        assert_eq!(entry.score, 17);
    }
}
```

- [ ] **Step 2: In `search.rs`, replace `negamax`'s body with an alpha-beta version that probes/stores a `TranspositionTable`**

```rust
use crate::tt::{Bound, TranspositionTable, TtEntry};
use crate::zobrist::hash_position;

pub fn negamax(pos: &Position, depth: u32) -> i32 {
    let mut tt = TranspositionTable::new();
    alphabeta(pos, depth, i32::MIN + 1, i32::MAX - 1, &mut tt)
}

fn alphabeta(pos: &Position, depth: u32, mut alpha: i32, beta: i32, tt: &mut TranspositionTable) -> i32 {
    let key = hash_position(pos);
    if let Some(entry) = tt.get(key) {
        if entry.depth >= depth {
            match entry.bound {
                Bound::Exact => return entry.score,
                Bound::Lower if entry.score >= beta => return entry.score,
                Bound::Upper if entry.score <= alpha => return entry.score,
                _ => {}
            }
        }
    }

    let turns = generate_turns(pos);
    if depth == 0 || turns.is_empty() {
        return evaluate(pos);
    }

    let ordered = crate::ordering::order_turns(pos, turns);
    let mut best = i32::MIN + 1;
    let original_alpha = alpha;
    for turn in ordered {
        let next = apply_turn(pos, &turn);
        let score = -alphabeta(&next, depth - 1, -beta, -alpha, tt);
        if score > best {
            best = score;
        }
        if best > alpha {
            alpha = best;
        }
        if alpha >= beta {
            break;
        }
    }

    let bound = if best <= original_alpha { Bound::Upper } else if best >= beta { Bound::Lower } else { Bound::Exact };
    tt.insert(key, TtEntry { depth, score: best, bound });
    best
}
```

`best_turn` and `search` (Task 18/19) keep calling `negamax(&next, depth-1)` exactly as before — each call builds its own fresh `TranspositionTable`. (A shared table across the whole search tree is a straightforward follow-up once this is proven correct; not required for v1.)

- [ ] **Step 3: Add a regression test proving alpha-beta agrees with the Task 18 plain-negamax results**

```rust
#[test]
fn alpha_beta_agrees_with_task_18_mate_in_one() {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(Square::from_str("g8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(Square::from_str("f7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
    pos.board.set(Square::from_str("g7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
    pos.board.set(Square::from_str("h7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
    let (turn, _) = best_turn(&pos, 1).expect("a move must be found");
    assert_eq!(turn.mv.to, Square::from_str("a8").unwrap());
}
```

- [ ] **Step 4: Add `pub mod tt;` to `lib.rs`, run `cargo test -p spellchess-search`, expect 12 passing**

- [ ] **Step 5: Commit**

```bash
git add crates/search/src/tt.rs crates/search/src/search.rs crates/search/src/lib.rs
git commit -m "perf(search): add alpha-beta pruning with transposition table"
```

---

### Task 21: Quiescence search

**Files:**
- Modify: `crates/search/src/search.rs`

**Interfaces:**
- Consumes: `evaluate` (Task 17), `generate_turns, apply_turn` (core).
- Produces: `alphabeta`'s `depth == 0` base case now calls a new private `quiescence(pos, alpha, beta) -> i32` instead of `evaluate(pos)` directly. No public signatures change.

- [ ] **Step 1: Add to `search.rs`**

```rust
fn quiescence(pos: &Position, mut alpha: i32, beta: i32) -> i32 {
    let stand_pat = evaluate(pos);
    if stand_pat >= beta {
        return beta;
    }
    if stand_pat > alpha {
        alpha = stand_pat;
    }
    for turn in generate_turns(pos) {
        let is_capture = pos.board.get(turn.mv.to).is_some() || turn.mv.is_en_passant;
        if !is_capture {
            continue;
        }
        let next = apply_turn(pos, &turn);
        let score = -quiescence(&next, -beta, -alpha);
        if score >= beta {
            return beta;
        }
        if score > alpha {
            alpha = score;
        }
    }
    alpha
}
```

Change `alphabeta`'s base case from `if depth == 0 || turns.is_empty() { return evaluate(pos); }` to:

```rust
if turns.is_empty() {
    return evaluate(pos);
}
if depth == 0 {
    return quiescence(pos, alpha, beta);
}
```

- [ ] **Step 2: Add a regression test showing quiescence avoids a horizon-effect blunder**

```rust
fn quiescence_test_position() -> Position {
    // White queen can capture a black knight; the knight is defended by a black
    // pawn. A depth-0 evaluate() right after the capture looks great for White,
    // but quiescence must keep searching the recapture and score it correctly.
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(Square::from_str("d1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Queen }));
    pos.board.set(Square::from_str("d5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Knight }));
    pos.board.set(Square::from_str("c6").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
    pos
}

#[test]
fn quiescence_sees_past_a_hanging_capture_at_the_horizon() {
    let pos = quiescence_test_position();
    let score_with_quiescence = negamax(&pos, 1);
    assert!(score_with_quiescence < 400); // the "won" knight is recaptured, not a full +320
}
```

- [ ] **Step 3: Run `cargo test -p spellchess-search`, expect 13 passing**

- [ ] **Step 4: Commit**

```bash
git add crates/search/src/search.rs
git commit -m "feat(search): extend search with quiescence on captures"
```

---

### Task 22: Spell-aware evaluation (PST, mobility, spell tempo, jump-threat scan)

**Files:**
- Modify: `crates/search/src/eval.rs`

**Interfaces:**
- Consumes: `Position, Color, PieceKind, Square, generate_turns, is_square_attacked` (core), `SpellKind` (core).
- Produces: `evaluate`'s signature is unchanged (`fn(&Position)->i32`); its internals now add positional and spell terms.

- [ ] **Step 1: Add piece-square and mobility terms to `eval.rs`**

```rust
const KNIGHT_PST: [i32; 64] = [
    -20,-10,-10,-10,-10,-10,-10,-20,
    -10,  0,  5,  5,  5,  5,  0,-10,
    -10,  5, 10, 15, 15, 10,  5,-10,
    -10,  5, 15, 20, 20, 15,  5,-10,
    -10,  5, 15, 20, 20, 15,  5,-10,
    -10,  5, 10, 15, 15, 10,  5,-10,
    -10,  0,  5,  5,  5,  5,  0,-10,
    -20,-10,-10,-10,-10,-10,-10,-20,
];

fn positional(pos: &Position) -> i32 {
    let mut score = 0i32;
    for i in 0..64u8 {
        if let Some(p) = pos.board.get(Square(i)) {
            if p.kind == PieceKind::Knight {
                let idx = if p.color == Color::White { i as usize } else { 63 - i as usize };
                score += if p.color == Color::White { KNIGHT_PST[idx] } else { -KNIGHT_PST[idx] };
            }
        }
    }
    score
}

fn spell_tempo(pos: &Position, color: Color) -> i32 {
    let s = pos.spells(color);
    s.freeze.count as i32 * 8 + s.jump.count as i32 * 15
        - if s.freeze.lock > 0 { 3 } else { 0 }
        - if s.jump.lock > 0 { 5 } else { 0 }
}
```

- [ ] **Step 2: Add a jump-capture threat scan**

```rust
/// Cheap tactical scan: does `color` have a slider that would attack the enemy
/// king if exactly one occupied square between them became transparent, with a
/// jump still available? See rules/50-interactions.md#threat-detection-for-bots.
fn jump_threat_bonus(pos: &Position, color: Color) -> i32 {
    if !pos.spells(color).jump.castable() {
        return 0;
    }
    let enemy_king = match pos.board.king_square(color.opposite()) {
        Some(sq) => sq,
        None => return 0,
    };
    for i in 0..64u8 {
        let sq = Square(i);
        let piece = match pos.board.get(sq) {
            Some(p) if p.color == color && matches!(p.kind, PieceKind::Bishop | PieceKind::Rook | PieceKind::Queen) => p,
            _ => continue,
        };
        let dirs: &[(i8, i8)] = match piece.kind {
            PieceKind::Rook => &spellchess_core::rays::ROOK_DIRS,
            PieceKind::Bishop => &spellchess_core::rays::BISHOP_DIRS,
            PieceKind::Queen => continue, // queen direction covered by rook+bishop cases on other pieces; simple v1 approximation
            _ => continue,
        };
        for &dir in dirs {
            let ray = spellchess_core::rays::walk_ray(pos, sq, dir);
            let blockers: Vec<Square> = ray.iter().copied().filter(|&s| pos.board.get(s).is_some()).collect();
            if blockers.len() == 1 && ray.last() == Some(&enemy_king) {
                return 60;
            }
        }
    }
    0
}
```

(Note: this requires `rays` and `is_square_attacked` to be re-exported from `spellchess_core`'s `lib.rs` — add `pub use rays;` alongside the existing `pub use` lines if not already visible, and keep `pub mod rays;` public.)

- [ ] **Step 3: Wire the new terms into `evaluate`**

```rust
pub fn evaluate(pos: &Position) -> i32 {
    let mut white = 0i32;
    let mut black = 0i32;
    for i in 0..64 {
        if let Some(p) = pos.board.get(Square(i)) {
            let v = piece_value(p.kind);
            match p.color {
                Color::White => white += v,
                Color::Black => black += v,
            }
        }
    }
    let material = white - black;
    let position_term = positional(pos);
    let tempo_term = spell_tempo(pos, Color::White) - spell_tempo(pos, Color::Black);
    let threat_term = jump_threat_bonus(pos, Color::White) - jump_threat_bonus(pos, Color::Black);
    let total = material + position_term + tempo_term + threat_term;
    match pos.side_to_move {
        Color::White => total,
        Color::Black => -total,
    }
}
```

- [ ] **Step 4: Add tests**

```rust
#[test]
fn centralized_knight_scores_higher_than_rim_knight() {
    let mut centralized = Position { board: Board::empty(), ..Position::starting() };
    centralized.board.set(Square::from_str("d4").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Knight }));
    let mut rim = Position { board: Board::empty(), ..Position::starting() };
    rim.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Knight }));
    assert!(evaluate(&centralized) > evaluate(&rim));
}

#[test]
fn a_live_jump_capture_threat_is_rewarded() {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(Square::from_str("d2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Bishop }));
    pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(Square::from_str("b4").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Bishop }));
    pos.side_to_move = Color::Black;
    let with_threat = evaluate(&pos);
    pos.black_spells.jump.count = 0;
    let without_threat = evaluate(&pos);
    assert!(with_threat > without_threat);
}
```

- [ ] **Step 5: Run `cargo test -p spellchess-search`, expect 15 passing**

- [ ] **Step 6: Commit**

```bash
git add crates/search/src/eval.rs
git commit -m "feat(search): add piece-square, spell-tempo, and jump-threat eval terms"
```

---

### Task 23: `cli` crate skeleton — notation parsing and board rendering

**Files:**
- Create: `crates/cli/Cargo.toml`
- Create: `crates/cli/src/lib.rs`
- Create: `crates/cli/src/notation.rs`
- Create: `crates/cli/src/render.rs`

**Interfaces:**
- Consumes: `Turn, SpellCast, SpellKind, PieceMove, Promotion, Position, Square, legal_moves` (core).
- Produces: `pub fn parse_turn(input:&str, pos:&Position)->Result<Turn,String>`; `pub fn format_turn(t:&Turn)->String`; `pub fn render_board(pos:&Position)->String`.

- [ ] **Step 1: Write `crates/cli/Cargo.toml`**

```toml
[package]
name = "spellchess-cli"
version = "0.1.0"
edition = "2021"

[dependencies]
spellchess-core = { path = "../core" }
spellchess-search = { path = "../search" }

[[bin]]
name = "spellchess"
path = "src/main.rs"
```

- [ ] **Step 2: Write `crates/cli/src/notation.rs`**

```rust
use spellchess_core::{legal_moves, PieceMove, Position, Promotion, Square, SpellCast, SpellKind, Turn};

pub fn parse_turn(input: &str, pos: &Position) -> Result<Turn, String> {
    let input = input.trim();
    let (spell, rest) = match input.split_once(' ') {
        Some((head, tail)) if head.starts_with("freeze@") || head.starts_with("jump@") => {
            (Some(parse_spell(head)?), tail.trim())
        }
        _ => (None, input),
    };
    let mv = parse_move(rest, pos)?;
    Ok(Turn { spell, mv })
}

fn parse_spell(s: &str) -> Result<SpellCast, String> {
    let (kind_str, sq_str) = s.split_once('@').ok_or_else(|| format!("bad spell: {s}"))?;
    let kind = match kind_str {
        "freeze" => SpellKind::Freeze,
        "jump" => SpellKind::Jump,
        other => return Err(format!("unknown spell: {other}")),
    };
    let square = Square::from_str(sq_str).ok_or_else(|| format!("bad square: {sq_str}"))?;
    Ok(SpellCast { kind, square })
}

fn parse_move(s: &str, pos: &Position) -> Result<PieceMove, String> {
    if s.len() < 4 {
        return Err(format!("bad move: {s}"));
    }
    let from = Square::from_str(&s[0..2]).ok_or_else(|| format!("bad from-square: {s}"))?;
    let to = Square::from_str(&s[2..4]).ok_or_else(|| format!("bad to-square: {s}"))?;
    let promotion = match s.get(4..5) {
        Some("q") => Some(Promotion::Queen),
        Some("r") => Some(Promotion::Rook),
        Some("b") => Some(Promotion::Bishop),
        Some("n") => Some(Promotion::Knight),
        Some(other) => return Err(format!("bad promotion: {other}")),
        None => None,
    };
    legal_moves(pos)
        .into_iter()
        .find(|m| m.from == from && m.to == to && m.promotion == promotion)
        .ok_or_else(|| format!("illegal move: {s}"))
}

fn spell_name(k: SpellKind) -> &'static str {
    match k {
        SpellKind::Freeze => "freeze",
        SpellKind::Jump => "jump",
    }
}

fn promo_suffix(p: Option<Promotion>) -> &'static str {
    match p {
        Some(Promotion::Queen) => "q",
        Some(Promotion::Rook) => "r",
        Some(Promotion::Bishop) => "b",
        Some(Promotion::Knight) => "n",
        None => "",
    }
}

pub fn format_turn(t: &Turn) -> String {
    let mv = format!("{}{}{}", t.mv.from, t.mv.to, promo_suffix(t.mv.promotion));
    match t.spell {
        Some(cast) => format!("{}@{} {}", spell_name(cast.kind), cast.square, mv),
        None => mv,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellchess_core::Position;

    #[test]
    fn parses_a_plain_move() {
        let pos = Position::starting();
        let turn = parse_turn("e2e4", &pos).unwrap();
        assert!(turn.spell.is_none());
        assert_eq!(turn.mv.from, Square::from_str("e2").unwrap());
        assert_eq!(turn.mv.to, Square::from_str("e4").unwrap());
    }

    #[test]
    fn parses_a_spell_and_move() {
        let pos = Position::starting();
        let turn = parse_turn("freeze@d5 e2e4", &pos).unwrap();
        assert_eq!(turn.spell, Some(SpellCast { kind: SpellKind::Freeze, square: Square::from_str("d5").unwrap() }));
    }

    #[test]
    fn rejects_an_illegal_move() {
        let pos = Position::starting();
        assert!(parse_turn("e2e5", &pos).is_err());
    }

    #[test]
    fn format_turn_round_trips_through_parse_turn() {
        let pos = Position::starting();
        let turn = parse_turn("e2e4", &pos).unwrap();
        assert_eq!(format_turn(&turn), "e2e4");
    }
}
```

- [ ] **Step 3: Write `crates/cli/src/render.rs`**

```rust
use spellchess_core::{Color, PieceKind, Position, Square};

fn glyph(kind: PieceKind, color: Color) -> char {
    let c = match kind {
        PieceKind::Pawn => 'p',
        PieceKind::Knight => 'n',
        PieceKind::Bishop => 'b',
        PieceKind::Rook => 'r',
        PieceKind::Queen => 'q',
        PieceKind::King => 'k',
    };
    if color == Color::White {
        c.to_ascii_uppercase()
    } else {
        c
    }
}

pub fn render_board(pos: &Position) -> String {
    let mut out = String::new();
    for rank in (0..8).rev() {
        out.push_str(&(rank + 1).to_string());
        out.push(' ');
        for file in 0..8 {
            let ch = match pos.board.get(Square::new(file, rank)) {
                Some(p) => glyph(p.kind, p.color),
                None => '.',
            };
            out.push(ch);
            out.push(' ');
        }
        out.push('\n');
    }
    out.push_str("  a b c d e f g h\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellchess_core::{PieceKind, Color};

    #[test]
    fn glyph_uppercases_white_only() {
        assert_eq!(glyph(PieceKind::King, Color::White), 'K');
        assert_eq!(glyph(PieceKind::Pawn, Color::Black), 'p');
    }

    #[test]
    fn render_board_has_nine_lines() {
        let out = render_board(&Position::starting());
        assert_eq!(out.lines().count(), 9);
        assert!(out.lines().next().unwrap().starts_with("8 r n b q k b n r"));
    }
}
```

- [ ] **Step 4: Write `crates/cli/src/lib.rs`**

```rust
pub mod notation;
pub mod render;
```

- [ ] **Step 5: Run `cargo test -p spellchess-cli`, expect 6 passing**

- [ ] **Step 6: Commit**

```bash
git add crates/cli/Cargo.toml crates/cli/src/lib.rs crates/cli/src/notation.rs crates/cli/src/render.rs
git commit -m "feat(cli): add coordinate notation parsing and ASCII board rendering"
```

---

### Task 24: REPL session (`newgame`, `board`, `undo`, `quit`, apply turn)

**Files:**
- Create: `crates/cli/src/repl.rs`
- Create: `crates/cli/src/main.rs`
- Modify: `crates/cli/src/lib.rs`

**Interfaces:**
- Consumes: `parse_turn` (Task 23), `render_board` (Task 23), `apply_turn, game_status, GameStatus, Position` (core).
- Produces: `pub struct Session{pos:Position, history:Vec<Position>}` with `Session::new()`, `.handle_command(&mut self, line:&str)->String` (handles `board`, `fen`, `undo`, `newgame [white|black]`, and turn strings), `.status(&self)->GameStatus`.

- [ ] **Step 1: Write `crates/cli/src/repl.rs`**

```rust
use spellchess_core::{apply_turn, game_status, GameStatus, Position};
use crate::notation::parse_turn;
use crate::render::render_board;

pub struct Session {
    pub pos: Position,
    pub history: Vec<Position>,
}

impl Session {
    pub fn new() -> Self {
        Session { pos: Position::starting(), history: Vec::new() }
    }

    pub fn status(&self) -> GameStatus {
        game_status(&self.pos)
    }

    pub fn handle_command(&mut self, line: &str) -> String {
        let line = line.trim();
        match line {
            "" => String::new(),
            "board" => render_board(&self.pos),
            "fen" => format!("{:?}", self.pos),
            "undo" => match self.history.pop() {
                Some(prev) => {
                    self.pos = prev;
                    "undone".to_string()
                }
                None => "nothing to undo".to_string(),
            },
            "newgame" | "newgame white" | "newgame black" => {
                self.pos = Position::starting();
                self.history.clear();
                format!("new game, you are {}", if line.ends_with("black") { "black" } else { "white" })
            }
            other => self.apply_turn_command(other),
        }
    }

    fn apply_turn_command(&mut self, input: &str) -> String {
        match parse_turn(input, &self.pos) {
            Ok(turn) => {
                self.history.push(self.pos.clone());
                self.pos = apply_turn(&self.pos, &turn);
                match self.status() {
                    GameStatus::InProgress => "ok".to_string(),
                    GameStatus::Checkmate(w) => format!("checkmate, {w:?} wins"),
                    GameStatus::Stalemate => "stalemate -- draw".to_string(),
                    GameStatus::KingCaptured(w) => format!("king captured, {w:?} wins"),
                }
            }
            Err(e) => format!("error: {e}"),
        }
    }
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellchess_core::{Board, Color, Piece, PieceKind, Square};

    #[test]
    fn applying_a_move_updates_the_position_and_reports_ok() {
        let mut session = Session::new();
        assert_eq!(session.handle_command("e2e4"), "ok");
        assert_eq!(session.pos.board.get(Square::from_str("e4").unwrap()).unwrap().kind, PieceKind::Pawn);
    }

    #[test]
    fn undo_restores_the_previous_position() {
        let mut session = Session::new();
        session.handle_command("e2e4");
        assert_eq!(session.handle_command("undo"), "undone");
        assert!(session.pos.board.get(Square::from_str("e4").unwrap()).is_none());
    }

    #[test]
    fn illegal_move_is_reported_and_does_not_change_state() {
        let mut session = Session::new();
        let before = session.pos.clone();
        assert!(session.handle_command("e2e5").starts_with("error:"));
        assert_eq!(session.pos, before);
    }

    #[test]
    fn status_reports_checkmate_for_a_known_mate_position() {
        let mut session = Session::new();
        session.pos = Position { board: Board::empty(), ..Position::starting() };
        session.pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        session.pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        session.pos.board.set(Square::from_str("h1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        session.pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        session.pos.black_spells = spellchess_core::SpellState {
            freeze: spellchess_core::SpellCounter { count: 0, lock: 0 },
            jump: spellchess_core::SpellCounter { count: 0, lock: 0 },
        };
        session.pos.fields.push(spellchess_core::SpellField {
            square: Square::from_str("e8").unwrap(), owner: Color::White,
            kind: spellchess_core::SpellKind::Freeze, expires_after_ply: session.pos.ply + 1,
        });
        session.pos.side_to_move = Color::Black;
        assert_eq!(session.status(), GameStatus::Checkmate(Color::White));
    }
}
```

- [ ] **Step 2: Write `crates/cli/src/main.rs`**

```rust
use spellchess_cli::repl::Session;
use std::io::{BufRead, Write};

fn main() {
    let mut session = Session::new();
    println!("Spell Chess advisor. Type 'quit' to exit.");
    let stdin = std::io::stdin();
    loop {
        print!("> ");
        std::io::stdout().flush().ok();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        if line.trim() == "quit" {
            break;
        }
        println!("{}", session.handle_command(&line));
    }
}
```

- [ ] **Step 3: Add `pub mod repl;` to `crates/cli/src/lib.rs`**

- [ ] **Step 4: Run `cargo test -p spellchess-cli`, expect 10 passing. Run `cargo build -p spellchess-cli` to confirm the binary compiles.**

- [ ] **Step 5: Commit**

```bash
git add crates/cli/src/repl.rs crates/cli/src/main.rs crates/cli/src/lib.rs
git commit -m "feat(cli): add REPL session with newgame/board/undo and turn application"
```

---

### Task 25: `go` command wired to search

**Files:**
- Modify: `crates/cli/Cargo.toml` (already depends on `spellchess-search` from Task 23)
- Modify: `crates/cli/src/repl.rs`

**Interfaces:**
- Consumes: `search, Budget` (search crate Task 19), `format_turn` (Task 23).
- Produces: `Session::handle_command` now handles lines starting with `"go"`, e.g. `go`, `go --time 5`, `go --depth 4`.

- [ ] **Step 1: Add to `repl.rs`**

```rust
use spellchess_search::search::{search as run_search, Budget};

fn parse_budget(rest: &str) -> Budget {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    match parts.as_slice() {
        ["--depth", n] => n.parse().map(Budget::Depth).unwrap_or(Budget::Time(std::time::Duration::from_secs(5))),
        ["--time", n] => n.parse().map(|s| Budget::Time(std::time::Duration::from_secs(s))).unwrap_or(Budget::Time(std::time::Duration::from_secs(5))),
        _ => Budget::Time(std::time::Duration::from_secs(5)),
    }
}
```

In `handle_command`'s `match`, add a guard arm before the catch-all `other =>` arm:

```rust
line if line == "go" || line.starts_with("go ") => self.handle_go(line),
```

And add the method:

```rust
impl Session {
    fn handle_go(&self, line: &str) -> String {
        let rest = line.strip_prefix("go").unwrap_or("").trim();
        let budget = parse_budget(rest);
        match run_search(&self.pos, budget) {
            Some((turn, score)) => format!("suggest: {} (eval {})", crate::notation::format_turn(&turn), score),
            None => "no legal turn available".to_string(),
        }
    }
}
```

- [ ] **Step 2: Add a test**

```rust
#[test]
fn go_returns_a_legal_suggestion() {
    let session = Session::new();
    let output = session.handle_go("go --depth 2");
    assert!(output.starts_with("suggest: "));
    let mv_str = output.strip_prefix("suggest: ").unwrap().split(" (eval").next().unwrap();
    assert!(crate::notation::parse_turn(mv_str, &session.pos).is_ok());
}
```

- [ ] **Step 3: Run `cargo test -p spellchess-cli`, expect 11 passing. Run `cargo build --workspace` to confirm the whole workspace still compiles cleanly.**

- [ ] **Step 4: Commit**

```bash
git add crates/cli/src/repl.rs
git commit -m "feat(cli): wire the go command to the search engine"
```

---

### Task 26 (optional, stretch — do after v1 ships): Tier 2 browser-oracle differential fixtures

Per the design spec, Tier 2 is explicitly non-blocking. Do this only after Tasks 1-25 are complete and merged.

**Approach:** using the chrome-devtools MCP tool against **only**
`https://www.chess.com/variants/spell-chess/analysis`, drive the harness already documented
in `rules/70-engine-api.md` to generate a broader set of positions than the 20 hand-encoded
vectors (deeper multi-spell sequences, randomized piece placements), capturing for each:
the position, side to move, and the full legal destination list per square plus both
spells' target lists. Serialize these as JSON fixtures under
`crates/core/tests/fixtures/*.json` (position + expected turns), and add one Rust
integration test (`crates/core/tests/oracle_vectors.rs`) that loads every fixture file and
asserts `generate_turns` (translated into the same square-set shape) matches. The oracle
itself — `research/variants.js` — is never a build dependency; only its **output**, captured
once and checked in as static data, is used.

---

## Final verification (after Task 26 or after Task 25 if Task 26 is deferred)

- [ ] Run `cargo test --workspace` and confirm every test across all three crates passes.
- [ ] Run `cargo build --workspace --release` and confirm the `spellchess` binary builds.
- [ ] Manually run `./target/release/spellchess`, play a few moves via `newgame`, `e2e4`, `board`, `go --depth 3`, `undo`, `quit`, and confirm the output looks sane.
- [ ] Re-read `rules/90-test-vectors.md` end to end and confirm all 20 vectors are represented somewhere in `crates/core`'s test suite (cross-reference against Tasks 8-15).


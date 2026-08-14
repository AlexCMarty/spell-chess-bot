use crate::bitboard::Bitboard;
use crate::position::Position;
use crate::types::Square;

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

/// Walks a ray from `from` in direction `dir`, stopping after the first occupied
/// square (inclusive). Occupancy already subtracts live jump squares, so the ray
/// continues through them as if they were empty.
pub fn walk_ray(pos: &Position, from: Square, dir: (i8, i8)) -> Vec<Square> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitboard::Bitboard;
    use crate::position::Position;
    use crate::types::{Color, Piece, PieceKind};

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

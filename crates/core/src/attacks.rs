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

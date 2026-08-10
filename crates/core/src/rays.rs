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
        if pos.board.get(sq).is_some() && !crate::spells::is_square_jump_active(pos, sq) {
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

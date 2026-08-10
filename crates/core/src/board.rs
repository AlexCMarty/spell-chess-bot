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

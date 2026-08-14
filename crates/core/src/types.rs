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

    pub fn index(self) -> usize {
        match self {
            Color::White => 0,
            Color::Black => 1,
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
}

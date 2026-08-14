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

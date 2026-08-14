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
            if old.kind == PieceKind::King && self.kings[old.color.index()] == Some(sq) {
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

    #[test]
    fn relocating_a_king_keeps_the_cache_in_either_order() {
        let king = Piece { color: Color::White, kind: PieceKind::King };
        let e1 = Square::from_str("e1").unwrap();
        let e2 = Square::from_str("e2").unwrap();

        let mut dest_then_origin = Board::empty();
        dest_then_origin.set(e1, Some(king));
        dest_then_origin.set(e2, Some(king));
        dest_then_origin.set(e1, None);
        assert_eq!(dest_then_origin.king_square(Color::White), Some(e2));
        assert_eq!(dest_then_origin.get(e2), Some(king));
        assert_eq!(dest_then_origin.get(e1), None);

        let mut origin_then_dest = Board::empty();
        origin_then_dest.set(e1, Some(king));
        origin_then_dest.set(e1, None);
        origin_then_dest.set(e2, Some(king));
        assert_eq!(origin_then_dest.king_square(Color::White), Some(e2));
        assert_eq!(origin_then_dest.get(e2), Some(king));
        assert_eq!(origin_then_dest.get(e1), None);
    }
}

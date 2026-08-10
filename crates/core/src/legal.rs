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
        v.dedup();
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
}

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
        pos.board.set(Square::from_str("h8").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
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

use spellchess_core::{apply_turn, generate_turns, Position, Turn};
use crate::eval::evaluate;

pub fn negamax(pos: &Position, depth: u32) -> i32 {
    let turns = generate_turns(pos);

    // King-capture terminal: the side to move has no king. Doesn't require
    // turns to be empty (the other side's remaining pieces can still move).
    let king_sq = match pos.board.king_square(pos.side_to_move) {
        Some(sq) => sq,
        None => return i32::MIN + 1, // loss for the side to move
    };

    if turns.is_empty() {
        // No legal turns: checkmate or stalemate. We already have king_sq,
        // so this is one attack check, not a second generate_turns call.
        return if spellchess_core::is_square_attacked(pos, king_sq, pos.side_to_move.opposite()) {
            i32::MIN + 1 // checkmated: loss for the side to move
        } else {
            0 // stalemate
        };
    }

    if depth == 0 {
        return evaluate(pos);
    }

    let mut best = i32::MIN;
    for turn in turns {
        let next = apply_turn(pos, &turn);
        let score = negamax(&next, depth - 1).saturating_neg();
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
        let score = negamax(&next, depth.saturating_sub(1)).saturating_neg();
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
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("b4").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Bishop }));
        pos.side_to_move = Color::Black;
        let (turn, _score) = best_turn(&pos, 1).expect("a move must be found");
        assert_eq!(turn.mv.to, Square::from_str("e1").unwrap());
        assert!(turn.spell.is_some());
    }
}

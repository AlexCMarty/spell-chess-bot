use spellchess_core::{Position, Turn};

pub fn order_turns(pos: &Position, mut turns: Vec<Turn>) -> Vec<Turn> {
    turns.sort_by_key(|t| std::cmp::Reverse(turn_priority(pos, t)));
    turns
}

fn turn_priority(pos: &Position, t: &Turn) -> i32 {
    let mut score = 0;
    if let Some(captured) = pos.board.get(t.mv.to) {
        score += 1000 + crate::eval::piece_value(captured.kind);
    }
    if t.spell.is_none() {
        score += 50; // cheap default: prefer a plain move over a speculative cast
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellchess_core::{generate_turns, Board, Color, Piece, PieceKind, Position, Square};

    #[test]
    fn ordering_preserves_the_turn_set() {
        let pos = Position::starting();
        let ordered = order_turns(&pos, generate_turns(&pos));
        assert_eq!(ordered.len(), generate_turns(&pos).len());
    }

    #[test]
    fn captures_sort_before_quiet_moves() {
        // White's queen on d1 can capture an undefended black knight on d5; every
        // other turn available is quiet. Ordering is load-bearing for alpha-beta's
        // pruning efficiency, so assert the real property: no quiet turn may appear
        // ahead of any capture.
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Queen }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Knight }));

        let turns = generate_turns(&pos);
        let ordered = order_turns(&pos, turns.clone());
        assert_eq!(ordered.len(), turns.len(), "ordering must preserve the turn set");

        let is_capture = |t: &Turn| pos.board.get(t.mv.to).is_some();
        let last_capture = ordered.iter().rposition(is_capture).expect("test position must offer a capture");
        let first_quiet = ordered.iter().position(|t| !is_capture(t)).expect("test position must offer quiet turns");
        assert!(
            last_capture < first_quiet,
            "every capture must sort ahead of every quiet turn (last capture at {last_capture}, first quiet at {first_quiet})",
        );
        assert_eq!(ordered[0].mv.to, Square::from_str("d5").unwrap(), "the capture should lead the list");
    }
}

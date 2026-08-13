use spellchess_core::{Position, Turn};
use crate::tables::HistoryTable;

const TT_MOVE_SCORE: i32 = 2_000_000;
const CAPTURE_BASE: i32 = 1_000_000;
const KILLER_SCORE: i32 = 500_000;
const HISTORY_CAP: i32 = 499_999;

pub fn order_turns(
    pos: &Position,
    mut turns: Vec<Turn>,
    tt_move: Option<Turn>,
    killers: [Option<Turn>; 2],
    history: Option<&HistoryTable>,
) -> Vec<Turn> {
    turns.sort_by_key(|t| std::cmp::Reverse(turn_priority(pos, t, tt_move, killers, history)));
    turns
}

fn turn_priority(
    pos: &Position,
    t: &Turn,
    tt_move: Option<Turn>,
    killers: [Option<Turn>; 2],
    history: Option<&HistoryTable>,
) -> i32 {
    if tt_move == Some(*t) {
        return TT_MOVE_SCORE;
    }
    let mut score = 0;
    let is_capture = pos.board.get(t.mv.to).is_some() || t.mv.is_en_passant;
    if is_capture {
        let captured_value = pos.board.get(t.mv.to).map(|p| crate::eval::piece_value(p.kind)).unwrap_or(0);
        score += CAPTURE_BASE + captured_value;
    } else if killers.contains(&Some(*t)) {
        score += KILLER_SCORE;
    } else if let Some(h) = history {
        score += (h.score(t.mv.from, t.mv.to) as i32).min(HISTORY_CAP);
    }
    if t.spell.is_none() {
        score += 50; // cheap default: prefer a plain move over a speculative cast
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables::HistoryTable;
    use spellchess_core::{generate_turns, Board, Color, Piece, PieceKind, Position, Square};

    #[test]
    fn ordering_preserves_the_turn_set() {
        let pos = Position::starting();
        let ordered = order_turns(&pos, generate_turns(&pos), None, [None, None], None);
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
        let ordered = order_turns(&pos, turns.clone(), None, [None, None], None);
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

    #[test]
    fn the_tt_move_sorts_first_even_ahead_of_a_capture() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Queen }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Knight }));

        let turns = generate_turns(&pos);
        let quiet_rook_move = turns
            .iter()
            .find(|t| t.mv.from == Square::from_str("a1").unwrap() && t.spell.is_none())
            .copied()
            .expect("a1 rook must have a quiet move available");

        let ordered = order_turns(&pos, turns, Some(quiet_rook_move), [None, None], None);
        assert_eq!(ordered[0], quiet_rook_move, "the TT move must sort first even though a capture is available");
    }

    #[test]
    fn a_killer_sorts_above_other_quiet_moves_but_below_a_capture() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Queen }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Knight }));

        let turns = generate_turns(&pos);
        let killer = turns
            .iter()
            .find(|t| t.mv.from == Square::from_str("a1").unwrap() && t.spell.is_none())
            .copied()
            .expect("a1 rook must have a quiet move available");

        let ordered = order_turns(&pos, turns, None, [Some(killer), None], None);
        let is_capture = |t: &Turn| pos.board.get(t.mv.to).is_some();
        let capture_count = ordered.iter().filter(|t| is_capture(t)).count();
        assert_eq!(ordered[capture_count], killer, "the killer must sort immediately after every capture");
    }

    #[test]
    fn a_higher_history_score_sorts_a_quiet_move_earlier() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("h1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));

        let turns = generate_turns(&pos);
        let a1_move = turns
            .iter()
            .find(|t| t.mv.from == Square::from_str("a1").unwrap() && t.mv.to == Square::from_str("a4").unwrap())
            .copied()
            .expect("a1 rook must be able to reach a4");
        let h1_move = turns
            .iter()
            .find(|t| t.mv.from == Square::from_str("h1").unwrap() && t.mv.to == Square::from_str("h4").unwrap())
            .copied()
            .expect("h1 rook must be able to reach h4");

        let mut history = HistoryTable::new();
        history.record(a1_move.mv.from, a1_move.mv.to, 5);

        let ordered = order_turns(&pos, turns, None, [None, None], Some(&history));
        let a1_pos = ordered.iter().position(|&t| t == a1_move).unwrap();
        let h1_pos = ordered.iter().position(|&t| t == h1_move).unwrap();
        assert!(a1_pos < h1_pos, "the move with history score must sort ahead of one with none");
    }
}

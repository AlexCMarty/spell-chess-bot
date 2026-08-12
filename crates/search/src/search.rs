use std::time::{Duration, Instant};
use spellchess_core::{apply_turn, generate_turns, Position, Turn};
use crate::eval::evaluate;
use crate::tt::{Bound, TranspositionTable, TtEntry};
use crate::zobrist::hash_position;

#[derive(Debug, Clone, Copy)]
pub enum Budget {
    Depth(u32),
    Time(Duration),
}

pub fn negamax(pos: &Position, depth: u32) -> i32 {
    let mut tt = TranspositionTable::new();
    alphabeta(pos, depth, i32::MIN + 1, i32::MAX - 1, &mut tt)
}

fn alphabeta(pos: &Position, depth: u32, mut alpha: i32, beta: i32, tt: &mut TranspositionTable) -> i32 {
    // King-capture terminal: cheap check, no move generation needed, checked
    // before the (comparatively expensive) TT hash so this fast path stays fast.
    let king_sq = match pos.board.king_square(pos.side_to_move) {
        Some(sq) => sq,
        None => return i32::MIN + 1, // loss for the side to move
    };

    let key = hash_position(pos);
    if let Some(entry) = tt.get(key) {
        if entry.depth >= depth {
            match entry.bound {
                Bound::Exact => return entry.score,
                Bound::Lower if entry.score >= beta => return entry.score,
                Bound::Upper if entry.score <= alpha => return entry.score,
                _ => {}
            }
        }
    }

    let turns = generate_turns(pos);
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

    let ordered = crate::ordering::order_turns(pos, turns);
    let mut best = i32::MIN + 1;
    let original_alpha = alpha;
    for turn in ordered {
        let next = apply_turn(pos, &turn);
        let score = -alphabeta(&next, depth - 1, -beta, -alpha, tt);
        if score > best {
            best = score;
        }
        if best > alpha {
            alpha = best;
        }
        if alpha >= beta {
            break;
        }
    }

    let bound = if best <= original_alpha { Bound::Upper } else if best >= beta { Bound::Lower } else { Bound::Exact };
    tt.insert(key, TtEntry { depth, score: best, bound });
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

pub fn search(pos: &Position, budget: Budget) -> Option<(Turn, i32)> {
    let start = Instant::now();
    let max_depth = match budget {
        Budget::Depth(d) => d,
        Budget::Time(_) => 64,
    };
    let mut best: Option<(Turn, i32)> = None;
    for depth in 1..=max_depth {
        if let Budget::Time(limit) = budget {
            if start.elapsed() >= limit {
                break;
            }
        }
        let turns = crate::ordering::order_turns(pos, generate_turns(pos));
        if turns.is_empty() {
            break;
        }
        let mut iter_best: Option<(Turn, i32)> = None;
        for turn in turns {
            let next = apply_turn(pos, &turn);
            let score = -negamax(&next, depth.saturating_sub(1));
            if iter_best.map_or(true, |(_, b)| score > b) {
                iter_best = Some((turn, score));
            }
            if let Budget::Time(limit) = budget {
                if start.elapsed() >= limit {
                    break;
                }
            }
        }
        if iter_best.is_some() {
            best = iter_best;
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
        pos.black_spells.freeze.count = 0; // isolate the jump-enabled king capture: without this, freeze@d2
                                            // also freezes White's king (d2's 3x3 zone includes e1) and Bxd2
                                            // delivers an independent, equally-winning checkmate that ties.
        let (turn, _score) = best_turn(&pos, 1).expect("a move must be found");
        assert_eq!(turn.mv.to, Square::from_str("e1").unwrap());
        assert!(turn.spell.is_some());
    }

    #[test]
    fn depth_budget_returns_a_move() {
        // A minimal, spell-free position: Position::starting() at Budget::Depth(2) is
        // combinatorially intractable pre-alpha-beta (freeze/jump target enumeration is
        // unfiltered per Tasks 9/11 — see the ledger). This test only needs to prove the
        // iterative-deepening loop in `search()` completes correctly across 2 plies; it
        // doesn't need to stress-test a fully-populated board (that's Task 20's job, once
        // pruning exists).
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.white_spells = spellchess_core::SpellState {
            freeze: spellchess_core::SpellCounter { count: 0, lock: 0 },
            jump: spellchess_core::SpellCounter { count: 0, lock: 0 },
        };
        pos.black_spells = pos.white_spells;
        assert!(search(&pos, Budget::Depth(2)).is_some());
    }

    #[test]
    fn time_budget_returns_within_the_budget() {
        let pos = Position::starting();
        let start = std::time::Instant::now();
        let result = search(&pos, Budget::Time(std::time::Duration::from_millis(200)));
        assert!(result.is_some());
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
    }

    #[test]
    fn alpha_beta_agrees_with_task_18_mate_in_one() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("g8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("f7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("g7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("h7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        let (turn, _) = best_turn(&pos, 1).expect("a move must be found");
        assert_eq!(turn.mv.to, Square::from_str("a8").unwrap());
    }
}

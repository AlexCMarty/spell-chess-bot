use std::time::{Duration, Instant};
use spellchess_core::{apply_turn, generate_search_turns, Position, Turn};
use crate::eval::evaluate;
use crate::tt::{Bound, TranspositionTable, TtEntry};
use crate::zobrist::hash_position;

#[derive(Debug, Clone, Copy)]
pub enum Budget {
    Depth(u32),
    Time(Duration),
}

/// Hard cap on how deep a capture-resolution search may run. Without it,
/// quiescence is unbounded and a single leaf can explode into millions of nodes.
const MAX_QUIESCENCE_DEPTH: u32 = 6;

pub fn negamax(pos: &Position, depth: u32) -> i32 {
    let mut tt = TranspositionTable::new();
    alphabeta(pos, depth, i32::MIN + 1, i32::MAX - 1, &mut tt, None)
        .expect("alphabeta with no deadline (None) never aborts")
}

/// `Option<i32>` here means `None` = the deadline was hit and the search must
/// unwind immediately; `Some(score)` is a real evaluated score. This propagates
/// through every recursive call via `match ... { Some(s) => -s, None => return None }`.
fn alphabeta(
    pos: &Position,
    depth: u32,
    mut alpha: i32,
    beta: i32,
    tt: &mut TranspositionTable,
    deadline: Option<Instant>,
) -> Option<i32> {
    if let Some(dl) = deadline {
        if Instant::now() >= dl {
            return None;
        }
    }

    // King-capture terminal: cheap check, no move generation needed, checked
    // before the (comparatively expensive) TT hash so this fast path stays fast.
    let king_sq = match pos.board.king_square(pos.side_to_move) {
        Some(sq) => sq,
        None => return Some(i32::MIN + 1), // loss for the side to move
    };

    let key = hash_position(pos);
    if let Some(entry) = tt.get(key) {
        if entry.depth >= depth {
            match entry.bound {
                Bound::Exact => return Some(entry.score),
                Bound::Lower if entry.score >= beta => return Some(entry.score),
                Bound::Upper if entry.score <= alpha => return Some(entry.score),
                _ => {}
            }
        }
    }

    let turns = generate_search_turns(pos);
    if turns.is_empty() {
        // No legal turns: checkmate or stalemate. We already have king_sq,
        // so this is one attack check, not a second generate_turns call.
        return Some(if spellchess_core::is_square_attacked(pos, king_sq, pos.side_to_move.opposite()) {
            i32::MIN + 1 // checkmated: loss for the side to move
        } else {
            0 // stalemate
        });
    }

    if depth == 0 {
        // Hand the already-generated turn list straight to quiescence rather than
        // making it call generate_turns again on the same position.
        return quiescence(pos, alpha, beta, MAX_QUIESCENCE_DEPTH, deadline, turns);
    }

    let ordered = crate::ordering::order_turns(pos, turns, None, [None, None], None);
    let mut best = i32::MIN + 1;
    let original_alpha = alpha;
    for turn in ordered {
        let next = apply_turn(pos, &turn);
        let score = match alphabeta(&next, depth - 1, -beta, -alpha, tt, deadline) {
            Some(s) => -s,
            None => return None,
        };
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
    tt.insert(key, TtEntry { depth, score: best, bound, best_move: None });
    Some(best)
}

/// Filters `turns` to captures and collapses duplicates that differ only by an
/// irrelevant spell cast (same from/to/promotion) down to one representative --
/// preferring the no-spell variant. `generate_turns` pairs every move with every
/// castable spell target, so without this a single real capture can appear
/// dozens of times, multiplying quiescence's effective branching factor.
fn dedup_captures(pos: &Position, turns: Vec<Turn>) -> Vec<Turn> {
    let mut out: Vec<Turn> = Vec::new();
    for turn in turns {
        let is_capture = pos.board.get(turn.mv.to).is_some() || turn.mv.is_en_passant;
        if !is_capture {
            continue;
        }
        match out.iter_mut().find(|t: &&mut Turn| {
            t.mv.from == turn.mv.from && t.mv.to == turn.mv.to && t.mv.promotion == turn.mv.promotion
        }) {
            Some(existing) => {
                if existing.spell.is_some() && turn.spell.is_none() {
                    *existing = turn;
                }
            }
            None => out.push(turn),
        }
    }
    out
}

fn quiescence(
    pos: &Position,
    mut alpha: i32,
    beta: i32,
    qdepth: u32,
    deadline: Option<Instant>,
    turns: Vec<Turn>,
) -> Option<i32> {
    if let Some(dl) = deadline {
        if Instant::now() >= dl {
            return None;
        }
    }
    if pos.board.king_square(pos.side_to_move).is_none() {
        return Some(i32::MIN + 1); // loss for the side to move: its king is already gone
    }

    let stand_pat = evaluate(pos);
    if stand_pat >= beta {
        return Some(beta);
    }
    if stand_pat > alpha {
        alpha = stand_pat;
    }
    if qdepth == 0 {
        return Some(alpha);
    }

    for turn in dedup_captures(pos, turns) {
        let next = apply_turn(pos, &turn);
        let next_turns = generate_search_turns(&next);
        let score = match quiescence(&next, -beta, -alpha, qdepth - 1, deadline, next_turns) {
            Some(s) => -s,
            None => return None,
        };
        if score >= beta {
            return Some(beta);
        }
        if score > alpha {
            alpha = score;
        }
    }
    Some(alpha)
}

pub fn best_turn(pos: &Position, depth: u32) -> Option<(Turn, i32)> {
    let turns = generate_search_turns(pos);
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
    let deadline = match budget {
        Budget::Time(limit) => Some(start + limit),
        Budget::Depth(_) => None,
    };
    let max_depth = match budget {
        Budget::Depth(d) => d,
        Budget::Time(_) => 64,
    };
    let mut tt = TranspositionTable::new();
    let mut best: Option<(Turn, i32)> = None;
    for depth in 1..=max_depth {
        if let Some(dl) = deadline {
            if Instant::now() >= dl {
                break;
            }
        }
        let turns = crate::ordering::order_turns(pos, generate_search_turns(pos), None, [None, None], None);
        if turns.is_empty() {
            break;
        }
        let mut iter_best: Option<(Turn, i32)> = None;
        let mut alpha = i32::MIN + 1;
        let beta = i32::MAX - 1;
        let mut complete = true;
        for turn in turns {
            let next = apply_turn(pos, &turn);
            let score = match alphabeta(&next, depth.saturating_sub(1), -beta, -alpha, &mut tt, deadline) {
                Some(s) => -s,
                None => {
                    complete = false;
                    break;
                }
            };
            if iter_best.map_or(true, |(_, b)| score > b) {
                iter_best = Some((turn, score));
            }
            if score > alpha {
                alpha = score;
            }
        }
        if complete {
            if iter_best.is_some() {
                best = iter_best;
            }
        } else {
            // Ran out of time mid-iteration: this iteration's ranking is biased
            // toward whatever prefix of the (capture-first-ordered) turn list got
            // evaluated before the deadline, so it isn't comparable to a complete
            // iteration's result -- discard it and keep the last complete
            // iteration's `best`. Exception: if no iteration has ever completed
            // (even depth 1 timed out), a partial ranking is still better than
            // returning None when legal moves clearly exist -- use it as a last
            // resort only in that case.
            if best.is_none() {
                best = iter_best;
            }
            break;
        }
    }

    // Deepest last resort: with a very short budget the deadline can fire before
    // even the first root move has been scored, leaving `best` empty. Returning
    // None means "no legal turn exists", which would be a lie here -- fall back to
    // the move `order_turns` ranks first, scored by a single static eval.
    if best.is_none() {
        if let Some(turn) = crate::ordering::order_turns(pos, generate_search_turns(pos), None, [None, None], None).into_iter().next() {
            let score = -evaluate(&apply_turn(pos, &turn));
            best = Some((turn, score));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellchess_core::{generate_turns, Board, Color, Piece, PieceKind, Position, Square};

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
        // A minimal, spell-free position, kept deliberately small: this test only needs
        // to prove the iterative-deepening loop in `search()` completes correctly across
        // 2 plies. `depth_three_search_completes_quickly_on_a_realistic_board` below is
        // the stress test on a fully-populated board, now that spell candidate generation
        // is relevance-filtered (see
        // docs/superpowers/specs/2026-08-12-search-branching-factor-design.md).
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
    fn time_budget_is_respected_on_a_full_board() {
        // The regression test for C1/C2/C4: on the full starting position the old
        // search ignored its time budget entirely (a 1s and a 5s budget both returned
        // after ~45s, because a single unbounded quiescence call had to finish before
        // the between-root-moves time check could fire). The deadline is now checked
        // at every node, so a 2s budget must actually return in about 2s.
        let pos = Position::starting();
        let start = std::time::Instant::now();
        let result = search(&pos, Budget::Time(Duration::from_secs(2)));
        let elapsed = start.elapsed();
        assert!(result.is_some(), "a legal turn exists in the starting position");
        assert!(
            elapsed < Duration::from_secs(5),
            "a 2s budget must not overrun by orders of magnitude, took {elapsed:?}",
        );
    }

    /// Regression test for the branching-factor fix: `go --depth N` is unbounded by
    /// design (Budget::Depth passes no deadline), and before relevance-filtered
    /// candidate generation, depth 1 alone measured ~54s on the fully-populated
    /// starting position. Measured after the fix in a release build: depth 1 ~2.8s,
    /// depth 2 ~33s -- the filter helps most in sparser positions (see the design doc),
    /// so the dense opening still isn't cheap at higher depths. This pins down the
    /// exact regression that was reported: depth 1 specifically must now be fast.
    ///
    /// The bound is tuned for a release build; debug-build overhead (no inlining, no
    /// bounds-check elision, more expensive allocation) swamps the algorithmic win here
    /// almost entirely -- a debug build measures ~53s, indistinguishable from the old
    /// unfiltered cost. Run with `cargo test -p spellchess-search --release -- --ignored`.
    #[test]
    #[ignore = "slow and misleading in a debug build; see doc comment"]
    fn depth_budget_stays_bounded_on_a_realistic_board() {
        let pos = Position::starting();
        let start = std::time::Instant::now();
        let result = search(&pos, Budget::Depth(1));
        let elapsed = start.elapsed();
        assert!(result.is_some(), "a legal turn exists in the starting position");
        assert!(
            elapsed < Duration::from_secs(15),
            "depth-1 search on a fully-populated board must stay well under the old ~54s, took {elapsed:?}",
        );
    }

    #[test]
    fn a_tiny_time_budget_still_returns_a_legal_turn() {
        // A budget too small to score even one root move must not report "no legal
        // turn available" on a position that plainly has ~1800 of them.
        let pos = Position::starting();
        for ms in [1u64, 20, 200] {
            let start = std::time::Instant::now();
            let (turn, _) = search(&pos, Budget::Time(Duration::from_millis(ms)))
                .unwrap_or_else(|| panic!("a {ms}ms budget must still yield a legal turn"));
            assert!(
                generate_turns(&pos).contains(&turn),
                "the fallback turn must be one the engine actually generated",
            );
            assert!(start.elapsed() < Duration::from_secs(2), "took {:?}", start.elapsed());
        }
    }

    fn quiescence_test_position() -> Position {
        // White queen can capture a black knight; the knight is defended by a black
        // pawn. A depth-0 evaluate() right after the capture looks great for White,
        // but quiescence must keep searching the recapture and score it correctly.
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Queen }));
        pos.board.set(Square::from_str("d5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Knight }));
        pos.board.set(Square::from_str("c6").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        // Disable spells so the only tactic on the board is the plain recapture on d5 —
        // otherwise White's queen capture can be paired with a freeze on c6 in the same
        // turn, which immobilizes the defending pawn and legitimately wins the knight for
        // free (score 800, confirmed by inspection). That's a real, correct tactical
        // finding, not a quiescence horizon-effect bug, and it would swamp the effect this
        // test exists to check. Same isolation pattern as `depth_budget_returns_a_move` and
        // `finds_king_capture_via_jump_in_one` above.
        pos.white_spells = spellchess_core::SpellState {
            freeze: spellchess_core::SpellCounter { count: 0, lock: 0 },
            jump: spellchess_core::SpellCounter { count: 0, lock: 0 },
        };
        pos.black_spells = pos.white_spells;
        pos
    }

    #[test]
    fn quiescence_sees_past_a_hanging_capture_at_the_horizon() {
        let pos = quiescence_test_position();
        let score_with_quiescence = negamax(&pos, 1);
        // Without quiescence (evaluate() called directly at depth 0), the search
        // blunders: it sees Qxd5 as winning a free knight (score 800, since the naive
        // leaf eval never looks past the capture to the c6 pawn's recapture) and plays
        // it over every safe alternative. With quiescence, Qxd5's true value collapses
        // once the recapture is searched out, so the engine instead keeps the queen
        // safe and preserves its pre-existing material edge (Queen vs Knight+Pawn),
        // landing at 480 — well under the naive blunder's 800. Threshold picked with a
        // wide margin between those two verified values, not tied to the exact 480.
        assert!(score_with_quiescence < 700);
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

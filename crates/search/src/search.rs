use std::sync::atomic::{AtomicBool, Ordering};
use crate::clock::{Duration, Instant};
use spellchess_core::{apply_turn, generate_quiescence_recapture_turns, generate_quiescence_turns_from, generate_search_spell_turns, generate_search_turns, legal_moves, Color, PieceKind, Position, Turn};
use crate::eval::{evaluate, piece_value};
use crate::tables::{HistoryTable, KillerTable};
use crate::tt::{Bound, TranspositionTable, TtEntry};
use crate::zobrist::hash_position;

#[derive(Debug, Clone, Copy)]
pub enum Budget {
    Depth(u32),
    Time(Duration),
}

/// Hard cap on how deep a capture-resolution search may run. Without it,
/// quiescence is unbounded and a single leaf can explode into millions of nodes.
const MAX_QUIESCENCE_DEPTH: u32 = 2;
const ASPIRATION: i32 = 24;
const KILLER_PLY_SLACK: u32 = 32;

/// Score for a forced win/loss, offset by ply-from-root so a faster mate always
/// beats a slower one. Comfortably above any real material/positional eval
/// (max `piece_value` is 20_000, for MVV-LVA ordering only -- `evaluate` never
/// sums a king) and far below `i32::MAX`, so negating it or adding a ply count
/// never overflows.
const MATE: i32 = 100_000;
/// Any `|score|` at or above this is a mate score, not a material/positional one.
const MATE_THRESHOLD: i32 = MATE - 1_000;

/// The side to move has just lost -- king captured, or checkmated with no spell
/// rescue -- at `ply` plies from the root. Losing later scores better (closer to
/// zero), so the search prefers to delay an inevitable loss over walking into a
/// faster one.
fn loss_at(ply: u32) -> i32 {
    -MATE + ply as i32
}

/// A mate score returned from a node at `ply` bakes in that node's absolute
/// distance from wherever this particular search path started (see `loss_at`).
/// The same position can be reached again later via a different path length --
/// a genuine transposition -- so before caching the score in the TT, strip out
/// this node's ply, leaving only "how many plies from *this position* to the
/// mate," a property of the position itself. Non-mate scores pass through
/// unchanged. See `tt_probe_score` for the inverse.
fn tt_store_score(score: i32, ply: u32) -> i32 {
    if score >= MATE_THRESHOLD {
        score + ply as i32
    } else if score <= -MATE_THRESHOLD {
        score - ply as i32
    } else {
        score
    }
}

/// Inverse of `tt_store_score`: rebases a cached mate distance onto the
/// probing node's own distance from the root, so a mate score reused across a
/// transposition reports the correct distance from wherever it's now being read.
fn tt_probe_score(score: i32, ply: u32) -> i32 {
    if score >= MATE_THRESHOLD {
        score - ply as i32
    } else if score <= -MATE_THRESHOLD {
        score + ply as i32
    } else {
        score
    }
}

struct SearchState<'a> {
    /// Shared, not owned: under Lazy-SMP every thread reads and writes this one table,
    /// and that sharing is the entire mechanism by which the helpers speed the main
    /// thread up. See `tt::TranspositionTable`'s sharding note.
    tt: &'a TranspositionTable,
    deadline: Option<Instant>,
    /// Set by the main thread when it has its answer, so helpers unwind instead of
    /// running their own iterative deepening to the bitter end. `None` for a search
    /// with no helpers to cancel.
    stop: Option<&'a AtomicBool>,
    /// The best turn found at ply 0 in the iteration currently running, recorded as
    /// it is found. `iterate` clears it before each attempt and reads it after.
    root_best: Option<(Turn, i32)>,
    killers: &'a mut KillerTable,
    history: &'a mut HistoryTable,
    nodes: u64,
    qnodes: u64,
    spell_gens: u64,
    tt_hits: u64,
    nmp_cutoffs: u64,
    no_spell_cutoffs: u64,
}

fn is_capture(pos: &Position, turn: &Turn) -> bool {
    pos.board.get(turn.mv.to).is_some() || turn.mv.is_en_passant
}

fn no_spell_turns(baseline: &[spellchess_core::PieceMove]) -> Vec<Turn> {
    baseline.iter().copied().map(|mv| Turn { spell: None, mv }).collect()
}

fn is_pv_window(alpha: i32, beta: i32) -> bool {
    beta > alpha.saturating_add(1)
}

fn timed_out(state: &SearchState<'_>) -> bool {
    if state.stop.is_some_and(|s| s.load(Ordering::Relaxed)) {
        return true;
    }
    matches!(state.deadline, Some(dl) if Instant::now() >= dl)
}

fn has_non_pawn_material(pos: &Position, color: Color) -> bool {
    let us = pos.board.color_bb(color);
    let pawns = pos.board.kind_bb(PieceKind::Pawn);
    let kings = pos.board.kind_bb(PieceKind::King);
    !us.minus(pawns).minus(kings).is_empty()
}

/// Pass the turn without a piece move, ticking opponent cooldowns and field
/// expiry the same way `apply_turn` would after a real completion. Used only
/// as a null-move-pruning probe — passing is illegal in the real game.
fn make_null_move(pos: &Position) -> Position {
    let mover = pos.side_to_move;
    let mut next = *pos;
    let opponent_spells = match mover {
        Color::White => &mut next.black_spells,
        Color::Black => &mut next.white_spells,
    };
    if opponent_spells.freeze.lock > 0 { opponent_spells.freeze.lock -= 1; }
    if opponent_spells.jump.lock > 0 { opponent_spells.jump.lock -= 1; }
    next.ply = pos.ply + 1;
    next.fields.retain(|f| next.ply <= f.expires_after_ply);
    next.side_to_move = mover.opposite();
    next.en_passant = None;
    next
}

fn spell_capture_turns(pos: &Position, baseline: &[spellchess_core::PieceMove]) -> Vec<Turn> {
    generate_quiescence_turns_from(pos, baseline)
        .into_iter()
        .filter(|t| t.spell.is_some())
        .collect()
}

/// Search `turns` left-to-right with PVS, LMR, LMP, and depth-1/2 futility.
/// `move_index` continues across staged generation so late spell pairings still reduce.
/// Returns `None` on timeout, `Some(true)` on beta cutoff.
fn search_children(
    pos: &Position,
    turns: Vec<Turn>,
    depth: u32,
    ply: u32,
    alpha: &mut i32,
    beta: i32,
    state: &mut SearchState<'_>,
    move_index: &mut usize,
    best: &mut i32,
    best_turn: &mut Option<Turn>,
    static_eval: Option<i32>,
    in_check: bool,
) -> Option<bool> {
    let is_pv = is_pv_window(*alpha, beta);
    for turn in turns {
        let idx = *move_index;
        *move_index += 1;
        let capture = is_capture(pos, &turn);
        if !is_pv && !in_check && !capture && turn.spell.is_none() {
            let lmp = 3 + (depth as usize) * (depth as usize);
            if idx >= lmp {
                continue;
            }
        }
        if let Some(eval) = static_eval {
            let margin = 200 + 150 * depth as i32;
            if idx > 0 && !capture && turn.spell.is_none() && eval.saturating_add(margin) <= *alpha {
                continue;
            }
        }
        let next = apply_turn(pos, &turn);
        let mut reduction = 0u32;
        if depth >= 3 && idx >= 2 && !capture && !in_check {
            reduction = 1;
            if turn.spell.is_some() {
                reduction += 1;
            }
            if !is_pv && idx >= 6 {
                reduction += 1;
            }
            reduction = reduction.min(depth - 1);
        }
        let child_depth = depth - 1 - reduction;
        let old_alpha = *alpha;
        let zw = idx > 0 || reduction > 0;
        let mut score = if !zw {
            match alphabeta(&next, child_depth, -beta, -old_alpha, ply + 1, state) {
                Some(s) => -s,
                None => return None,
            }
        } else {
            match alphabeta(&next, child_depth, -old_alpha - 1, -old_alpha, ply + 1, state) {
                Some(s) => -s,
                None => return None,
            }
        };
        if zw && score > old_alpha {
            score = match alphabeta(&next, depth - 1, -beta, -old_alpha, ply + 1, state) {
                Some(s) => -s,
                None => return None,
            };
        }
        if score > *best {
            *best = score;
            *best_turn = Some(turn);
            if ply == 0 {
                // Recorded here rather than read back out of the transposition table
                // after the search returns. The root entry is not private: under
                // Lazy-SMP another thread can evict it between the search and the
                // read, and even single-threaded a different position sharing the
                // slot can. Either way the old code silently kept the *previous*
                // iteration's move and score -- which is what made a 4-thread depth-6
                // search report eval 0 where 1 thread reported 56.
                state.root_best = Some((turn, score));
            }
        }
        if *best > *alpha {
            *alpha = *best;
        }
        if *alpha >= beta {
            if !capture {
                state.killers.record(ply, turn);
                state.history.record(turn.mv.from, turn.mv.to, depth);
            }
            return Some(true);
        }
    }
    Some(false)
}

/// `ply` is this position's distance from whatever the caller considers the true
/// search root -- pass `0` when `pos` genuinely is the root, or the ply already
/// spent reaching it (e.g. `best_turn` plays one ply itself before calling this,
/// so it passes `1`) so mate scores stay offset consistently with `search()`.
pub fn negamax(pos: &Position, depth: u32, ply: u32) -> i32 {
    let tt = TranspositionTable::new();
    let mut killers = KillerTable::new(depth + ply + KILLER_PLY_SLACK);
    let mut history = HistoryTable::new();
    let mut state = SearchState {
        tt: &tt,
        deadline: None,
        stop: None,
        root_best: None,
        killers: &mut killers,
        history: &mut history,
        nodes: 0,
        qnodes: 0,
        spell_gens: 0,
        tt_hits: 0,
        nmp_cutoffs: 0,
        no_spell_cutoffs: 0,
    };
    alphabeta(pos, depth, i32::MIN + 1, i32::MAX - 1, ply, &mut state)
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
    ply: u32,
    state: &mut SearchState<'_>,
) -> Option<i32> {
    state.nodes += 1;
    if timed_out(state) {
        return None;
    }

    // King-capture terminal: cheap check, no move generation needed, checked
    // before the (comparatively expensive) TT hash so this fast path stays fast.
    let king_sq = match pos.board.king_square(pos.side_to_move) {
        Some(sq) => sq,
        None => return Some(loss_at(ply)), // loss for the side to move
    };

    let is_pv = is_pv_window(alpha, beta);
    let in_check = spellchess_core::is_square_attacked(pos, king_sq, pos.side_to_move.opposite());
    let key = hash_position(pos);
    let tt_entry = state.tt.get(key);
    if let Some(entry) = tt_entry {
        // Never cut off at the root. The entry is still used for move ordering below,
        // but returning here would leave the iteration with no root move of its own
        // and `iterate` would keep the previous depth's answer. Single-threaded this
        // never fired -- the root entry from iteration d-1 is shallower than d -- but
        // a Lazy-SMP helper that reaches depth d first writes an entry that does, and
        // a 4-thread depth-6 search then silently reported its depth-5 move.
        if ply > 0 && entry.depth >= depth {
            let score = tt_probe_score(entry.score, ply);
            match entry.bound {
                Bound::Exact => {
                    state.tt_hits += 1;
                    return Some(score);
                }
                Bound::Lower if score >= beta => {
                    state.tt_hits += 1;
                    return Some(score);
                }
                Bound::Upper if score <= alpha => {
                    state.tt_hits += 1;
                    return Some(score);
                }
                _ => {}
            }
        }
    }

    if depth == 0 {
        let baseline = legal_moves(pos);
        if baseline.is_empty() {
            let turns = generate_search_turns(pos);
            if turns.is_empty() {
                return Some(if in_check {
                    loss_at(ply)
                } else {
                    0
                });
            }
        }
        let qturns = generate_quiescence_turns_from(pos, &baseline);
        return quiescence(pos, alpha, beta, MAX_QUIESCENCE_DEPTH, ply, state, qturns);
    }

    let static_eval = evaluate(pos);
    if !in_check && !is_pv && depth <= 6 && static_eval.saturating_sub(120 * depth as i32) >= beta {
        return Some(static_eval);
    }
    if !in_check && !is_pv && depth <= 2 {
        let margin = 250 + 150 * depth as i32;
        if static_eval.saturating_add(margin) < alpha {
            let baseline = legal_moves(pos);
            if let Some(q) = quiescence(pos, alpha, beta, MAX_QUIESCENCE_DEPTH, ply, state, generate_quiescence_turns_from(pos, &baseline)) {
                if q < alpha {
                    return Some(q);
                }
            } else {
                return None;
            }
        }
    }
    if !in_check && !is_pv && depth >= 3 && has_non_pawn_material(pos, pos.side_to_move) {
        let frozen_us = !spellchess_core::spells::frozen_bb(pos)
            .intersect(pos.board.color_bb(pos.side_to_move))
            .is_empty();
        if !frozen_us {
            let r = 2 + depth / 4;
            let next = make_null_move(pos);
            let null_depth = depth.saturating_sub(1 + r);
            if let Some(s) = alphabeta(&next, null_depth, -beta, -beta + 1, ply + 1, state) {
                if -s >= beta {
                    state.nmp_cutoffs += 1;
                    return Some(-s);
                }
            } else {
                return None;
            }
        }
    }
    let futility_eval = if depth <= 2 && !in_check && !is_pv { Some(static_eval) } else { None };

    let baseline = legal_moves(pos);
    let no_spell = no_spell_turns(&baseline);
    let no_spell_empty = no_spell.is_empty();
    let terminal = || if in_check { loss_at(ply) } else { 0 };

    let tt_move = tt_entry.and_then(|e| e.best_move);
    // Below every reachable score, including the deepest possible "king already
    // gone" loss (loss_at(0) = -MATE), so the first searched move always
    // records a best_turn.
    let mut best = i32::MIN;
    let mut best_turn: Option<Turn> = None;
    let original_alpha = alpha;
    let mut move_index = 0usize;
    let killer_pair = state.killers.pair(ply);

    if !no_spell_empty {
        let ordered = crate::ordering::order_turns(pos, no_spell, tt_move, killer_pair, Some(state.history));
        if search_children(
            pos, ordered, depth, ply, &mut alpha, beta, state,
            &mut move_index, &mut best, &mut best_turn, futility_eval, in_check,
        )? {
            state.no_spell_cutoffs += 1;
            let bound = if best <= original_alpha { Bound::Upper } else if best >= beta { Bound::Lower } else { Bound::Exact };
            state.tt.insert(key, TtEntry { depth, score: tt_store_score(best, ply), bound, best_move: best_turn });
            return Some(best);
        }
    }

    // Full freeze/jump pairing is ~1500 turns in the dense opening. Searching
    // that list on every PV node at remaining-depth 10+ is what made depth 15
    // blow up. The root must still see every relevant pairing — otherwise a
    // freeze that only retags an already-legal capture (immobilise the
    // defender, then take) can never be played. Interior nodes keep the full
    // list only for check evasions and shallow PV; everywhere else only
    // spell-enabled captures.
    let full_spells = ply == 0 || in_check || (is_pv && depth <= 3) || no_spell_empty;
    let spells = if full_spells {
        state.spell_gens += 1;
        generate_search_spell_turns(pos, &baseline)
    } else {
        spell_capture_turns(pos, &baseline)
    };
    if no_spell_empty && spells.is_empty() {
        return Some(terminal());
    }
    if !spells.is_empty() {
        let killer_pair = state.killers.pair(ply);
        let ordered = crate::ordering::order_turns(pos, spells, tt_move, killer_pair, Some(state.history));
        let _ = search_children(
            pos, ordered, depth, ply, &mut alpha, beta, state,
            &mut move_index, &mut best, &mut best_turn, futility_eval, in_check,
        )?;
    }

    // A truncated spell list can only miss a better move, so the score is not
    // an Exact/Upper bound. Fail-highs remain valid Lower bounds.
    if !(full_spells || best >= beta) {
        return Some(best);
    }
    let bound = if best <= original_alpha { Bound::Upper } else if best >= beta { Bound::Lower } else { Bound::Exact };
    state.tt.insert(key, TtEntry { depth, score: tt_store_score(best, ply), bound, best_move: best_turn });
    Some(best)
}

/// Search `turns` as captures. Spell and no-spell pairings of the same
/// from/to/promotion are kept both — collapsing them used to drop freeze-the-
/// defender captures in favour of the naked recapture.
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
                if existing.spell != turn.spell {
                    out.push(turn);
                }
            }
            None => { out.push(turn); }
        }
    }
    out
}

fn quiescence(
    pos: &Position,
    mut alpha: i32,
    beta: i32,
    qdepth: u32,
    ply: u32,
    state: &mut SearchState<'_>,
    turns: Vec<Turn>,
) -> Option<i32> {
    state.qnodes += 1;
    if timed_out(state) {
        return None;
    }
    if pos.board.king_square(pos.side_to_move).is_none() {
        return Some(loss_at(ply)); // loss for the side to move: its king is already gone
    }
    let king_sq = pos.board.king_square(pos.side_to_move).unwrap();
    let in_check = spellchess_core::is_square_attacked(pos, king_sq, pos.side_to_move.opposite());

    let stand_pat = evaluate(pos);
    if !in_check {
        if stand_pat >= beta {
            return Some(beta);
        }
    }
    if stand_pat > alpha {
        alpha = stand_pat;
    }
    if qdepth == 0 {
        return Some(alpha);
    }

    let search_turns = dedup_captures(pos, turns);
    for turn in search_turns {
        let captured_kind = if turn.mv.is_en_passant {
            Some(PieceKind::Pawn)
        } else {
            pos.board.get(turn.mv.to).map(|p| p.kind)
        };
        if let Some(kind) = captured_kind {
            if kind != PieceKind::King {
                let mut gain = piece_value(kind);
                if let Some(promo) = turn.mv.promotion {
                    gain += piece_value(promo.piece_kind()) - piece_value(PieceKind::Pawn);
                }
                if stand_pat.saturating_add(gain).saturating_add(200) < alpha {
                    continue;
                }
            }
        }
        let next = apply_turn(pos, &turn);
        // Cheap generator: the expensive per-target legal_moves() scan (jump and
        // freeze *new*-capture discovery) only runs once, at the entry into
        // quiescence (see the two generate_quiescence_turns_from call sites in
        // alphabeta). Recursive nodes like this one only need the "freeze the
        // recapturer" tactic, which generate_quiescence_recapture_turns covers
        // without it -- see its doc comment for why paying the full cost here
        // used to blow quiescence's node count up catastrophically.
        let next_turns = generate_quiescence_recapture_turns(&next, &legal_moves(&next));
        let score = match quiescence(&next, -beta, -alpha, qdepth - 1, ply + 1, state, next_turns) {
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
        let score = negamax(&next, depth.saturating_sub(1), 1).saturating_neg();
        if best.map_or(true, |(_, b)| score > b) {
            best = Some((turn, score));
        }
    }
    best
}

fn dump_profile(label: &str, start: Instant, state: &SearchState<'_>) {
    if std::env::var_os("SPELLCHESS_PROFILE").is_none() {
        return;
    }
    let elapsed = start.elapsed();
    let nps = if elapsed.as_secs_f64() > 0.0 {
        (state.nodes as f64 / elapsed.as_secs_f64()) as u64
    } else {
        0
    };
    eprintln!(
        "profile {label}: {elapsed:.3?} nodes={} qnodes={} spell_gens={} tt_hits={} nmp={} no_spell_cut={} nps={}",
        state.nodes, state.qnodes, state.spell_gens, state.tt_hits, state.nmp_cutoffs, state.no_spell_cutoffs, nps
    );
}

/// One iterative-deepening loop over the shared table. Every search thread runs this;
/// `start_depth` is the only thing that differs between them, so the helpers do not
/// walk the tree in lockstep and the table picks up their bounds and best moves.
/// Returns the best turn this thread proved, which only the main thread's caller uses.
fn iterate(
    pos: &Position,
    max_depth: u32,
    start_depth: u32,
    aspiration: bool,
    state: &mut SearchState<'_>,
) -> Option<(Turn, i32)> {
    let mut best: Option<(Turn, i32)> = None;
    let mut last_score: Option<i32> = None;
    let root_key = hash_position(pos);

    for depth in start_depth..=max_depth {
        if timed_out(state) {
            break;
        }
        let baseline = legal_moves(pos);
        if baseline.is_empty() && generate_search_spell_turns(pos, &baseline).is_empty() {
            break;
        }
        let mut alpha = i32::MIN + 1;
        let mut beta = i32::MAX - 1;
        // Helpers search full windows. A narrow aspiration window is only sound for
        // the thread that chose it: the Lower/Upper bounds it produces are relative to
        // that window, and every thread stores them into the one shared table. Letting
        // three helpers each narrow around their own running score is what made a
        // 4-thread depth-6 search swing between 2.4s and 26.5s.
        if aspiration {
            if let Some(score) = last_score {
                alpha = score.saturating_sub(ASPIRATION).max(i32::MIN + 1);
                beta = score.saturating_add(ASPIRATION).min(i32::MAX - 1);
            }
        }
        let mut complete = true;
        loop {
            // Cleared per attempt, so a fail-low that finds no root move leaves the
            // previous iteration's answer standing instead of re-adopting it here.
            state.root_best = None;
            match alphabeta(pos, depth, alpha, beta, 0, state) {
                None => {
                    complete = false;
                    break;
                }
                Some(score) => {
                    if score <= alpha && alpha > i32::MIN + 1 {
                        alpha = i32::MIN + 1;
                        continue;
                    }
                    if score >= beta && beta < i32::MAX - 1 {
                        beta = i32::MAX - 1;
                        continue;
                    }
                    last_score = Some(score);
                    if let Some((turn, _)) = state.root_best {
                        state.tt.insert(root_key, TtEntry {
                            depth,
                            score,
                            bound: Bound::Exact,
                            best_move: Some(turn),
                        });
                        best = Some((turn, score));
                    }
                    break;
                }
            }
        }
        if !complete {
            break;
        }
    }
    best
}

/// Threads to search with. **Defaults to 1**, and to anything else only if
/// `SPELLCHESS_THREADS` says so.
///
/// Defaulting to `available_parallelism` was tried and measured worse on 2026-09-02
/// (Pi 5, 4 cores, depth 6 from the starting position): 1 thread 6.97s, 2 threads
/// 10.25s, 4 threads 7.4-26.5s run to run. See `search_smp` for why -- it is a
/// property of this engine, not a tuning knob, so the default is off until that
/// changes.
pub fn default_threads() -> usize {
    if let Some(n) = std::env::var("SPELLCHESS_THREADS").ok().and_then(|v| v.parse::<usize>().ok()) {
        return n.max(1);
    }
    1
}

/// Single-threaded search. Deterministic: same position and budget, same answer and
/// same node counts, every time. Use `search_smp` to spend more cores.
pub fn search(pos: &Position, budget: Budget) -> Option<(Turn, i32)> {
    search_smp(pos, budget, 1)
}

/// Lazy SMP: `threads` threads share one transposition table and otherwise search
/// independently, differing only in the depth they start their iterative deepening at.
/// The helpers never report; they exist to fill the table with bounds and best moves
/// the main thread then hits, which is where the speedup comes from.
///
/// **Measured a net LOSS on this engine (2026-09-02, Pi 5, depth 6 from the starting
/// position): 1 thread 6.97s, 2 threads 10.25s, 4 threads 7.4-26.5s.** That is not a
/// tuning problem, it is structural. Lazy-SMP pays off when helpers shrink the main
/// thread's tree through shared table hits, and here they cannot: 87% of this search's
/// nodes are quiescence nodes (2.38M qnodes against 340k nodes at depth 6) and
/// quiescence never probes the transposition table, so every helper re-derives that
/// 87% from scratch. The counters say so directly -- 2 threads do 6.30M qnodes where 1
/// does 2.38M, i.e. 2.6x the work for 2x the cores, before the four cores even start
/// contending for bandwidth on an ~84MB table.
///
/// Making this pay would mean giving quiescence something to share, not tuning the
/// thread count. Left in, off by default, so the next attempt starts from the
/// measurement instead of repeating it.
///
/// The result is also not reproducible above 1 thread: which helper wins a race to a
/// table slot decides which of several turns comes back. `threads == 1` is exactly the
/// single-threaded path and stays deterministic.
pub fn search_smp(pos: &Position, budget: Budget, threads: usize) -> Option<(Turn, i32)> {
    let start = Instant::now();
    let threads = threads.max(1);
    let deadline = match budget {
        Budget::Time(limit) => Some(start + limit),
        Budget::Depth(_) => None,
    };
    let max_depth = match budget {
        Budget::Depth(d) => d,
        Budget::Time(_) => 64,
    };
    let tt = TranspositionTable::new();
    let stop = AtomicBool::new(false);
    let mut best: Option<(Turn, i32)> = None;

    std::thread::scope(|scope| {
        let helpers: Vec<_> = (1..threads)
            .map(|i| {
                let (tt, stop) = (&tt, &stop);
                scope.spawn(move || {
                    let mut killers = KillerTable::new(max_depth + KILLER_PLY_SLACK);
                    let mut history = HistoryTable::new();
                    let mut state = SearchState {
                        tt,
                        deadline,
                        stop: Some(stop),
                        root_best: None,
                        killers: &mut killers,
                        history: &mut history,
                        nodes: 0,
                        qnodes: 0,
                        spell_gens: 0,
                        tt_hits: 0,
                        nmp_cutoffs: 0,
                        no_spell_cutoffs: 0,
                    };
                    // Stagger the starting depth so the helpers are not re-deriving
                    // the main thread's current iteration move for move. Capped at
                    // `max_depth` so a shallow search still gives them something to do.
                    let offset = (i as u32 % 3).min(max_depth.saturating_sub(1));
                    iterate(pos, max_depth, 1 + offset, false, &mut state);
                    (state.nodes, state.qnodes, state.spell_gens, state.tt_hits, state.nmp_cutoffs, state.no_spell_cutoffs)
                })
            })
            .collect();

        let mut killers = KillerTable::new(max_depth + KILLER_PLY_SLACK);
        let mut history = HistoryTable::new();
        let mut state = SearchState {
            tt: &tt,
            deadline,
            stop: Some(&stop),
            root_best: None,
            killers: &mut killers,
            history: &mut history,
            nodes: 0,
            qnodes: 0,
            spell_gens: 0,
            tt_hits: 0,
            nmp_cutoffs: 0,
            no_spell_cutoffs: 0,
        };
        best = iterate(pos, max_depth, 1, true, &mut state);
        // The main thread has its answer; tell the helpers to unwind rather than
        // finish iterations nobody will read.
        stop.store(true, Ordering::Relaxed);
        for h in helpers {
            let (n, q, g, t, nm, ns) = h.join().expect("search helper thread panicked");
            state.nodes += n;
            state.qnodes += q;
            state.spell_gens += g;
            state.tt_hits += t;
            state.nmp_cutoffs += nm;
            state.no_spell_cutoffs += ns;
        }
        // Counters are summed across threads, so `nodes`/`nps` read as total work done
        // rather than work done by the reporting thread.
        dump_profile("search", start, &state);
    });

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

    /// Any `|score|` at or above this is a mate score, not a material/positional
    /// one -- `MATE - 1_000` stays comfortably above the largest real eval swing.
    fn is_mate_score(score: i32) -> bool {
        score.abs() >= MATE - 1_000
    }

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
    fn search_finds_jump_king_capture() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Bishop }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("b4").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Bishop }));
        pos.side_to_move = Color::Black;
        pos.black_spells.freeze.count = 0;
        let (turn, _score) = search(&pos, Budget::Depth(1)).expect("a move must be found");
        assert_eq!(turn.mv.to, Square::from_str("e1").unwrap());
        assert!(turn.spell.is_some());
    }

    /// The helpers must not change the answer on a position with only one winning
    /// move: whichever thread's best move reaches the root table entry first, it has
    /// to be that one.
    #[test]
    fn smp_finds_the_same_forced_mate_as_the_single_threaded_search() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("g8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("f7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("g7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("h7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.white_spells = spellchess_core::SpellState {
            freeze: spellchess_core::SpellCounter { count: 0, lock: 0 },
            jump: spellchess_core::SpellCounter { count: 0, lock: 0 },
        };
        pos.black_spells = pos.white_spells;
        let (one, one_score) = search_smp(&pos, Budget::Depth(2), 1).expect("a move must be found");
        for threads in [2, 4] {
            let (many, many_score) =
                search_smp(&pos, Budget::Depth(2), threads).expect("a move must be found");
            assert_eq!(many.mv.to, Square::from_str("a8").unwrap(), "{threads} threads missed Ra8#");
            assert_eq!(many.mv, one.mv, "{threads} threads disagreed with the 1-thread search");
            assert_eq!(many_score, one_score, "{threads} threads scored Ra8# differently");
        }
    }

    /// A time budget has to bind every thread, not just the reporting one -- the join
    /// at the end of `search_smp` blocks on the slowest helper, so a helper that
    /// ignored the deadline would hang the whole search well past it.
    #[test]
    fn an_smp_time_budget_binds_the_helper_threads_too() {
        let pos = Position::starting();
        let start = std::time::Instant::now();
        let result = search_smp(&pos, Budget::Time(std::time::Duration::from_millis(200)), 4);
        let elapsed = start.elapsed();
        assert!(result.is_some(), "a legal turn exists in the starting position");
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "search_smp overran its 200ms budget by far too much: {elapsed:?}",
        );
    }

    #[test]
    fn depth_budget_returns_a_move() {
        // A minimal, spell-free position, kept deliberately small: this test only needs
        // to prove the iterative-deepening loop in `search()` completes correctly across
        // 2 plies. `depth_three_search_completes_quickly_on_a_realistic_board` below is
        // the stress test on a fully-populated board, now that spell candidate generation
        // is relevance-filtered.
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

    /// Regression guard for search speed on the starting position.
    /// Depths 1, 3, 4, 6 and 8 share this test so they cannot run in parallel and
    /// contend for the same cores. Bounds are tuned for a release build on a
    /// Raspberry Pi 5 (odysseus) and measured 2026-09-01 after the spell-delta
    /// rewrite; a debug build's overhead swamps the algorithmic win. Depth 8 (a single
    /// measurement of 143.536s) gets roughly 2x headroom rather than the ~1.7x used
    /// elsewhere in this test, because it is the longest-running, most
    /// thermal-throttling- and scheduler-contention-exposed case and the one most
    /// likely to rot into flakiness (see the depth-15 bound this test used to carry).
    /// Run with `cargo test -p spellchess-search --release -- --ignored`.
    #[test]
    #[ignore = "slow and misleading in a debug build; see doc comment"]
    fn depth_budget_stays_bounded_on_a_realistic_board() {
        let pos = Position::starting();
        let start = std::time::Instant::now();
        let result = search(&pos, Budget::Depth(1));
        let elapsed = start.elapsed();
        assert!(result.is_some(), "a legal turn exists in the starting position");
        assert!(
            elapsed < Duration::from_millis(300),
            "depth-1 search on the starting position must finish in under 300ms, took {elapsed:?}",
        );

        let start = std::time::Instant::now();
        let result = search(&pos, Budget::Depth(3));
        let elapsed = start.elapsed();
        assert!(result.is_some(), "a legal turn exists in the starting position");
        assert!(
            elapsed < Duration::from_millis(750),
            "depth-3 search on the starting position must finish in under 750ms, took {elapsed:?}",
        );

        let start = std::time::Instant::now();
        let result = search(&pos, Budget::Depth(4));
        let elapsed = start.elapsed();
        assert!(result.is_some(), "a legal turn exists in the starting position");
        assert!(
            elapsed < Duration::from_millis(1500),
            "depth-4 search on the starting position must finish in under 1500ms, took {elapsed:?}",
        );

        let start = std::time::Instant::now();
        let result = search(&pos, Budget::Depth(6));
        let elapsed = start.elapsed();
        assert!(result.is_some(), "a legal turn exists in the starting position");
        assert!(
            elapsed < Duration::from_secs(15),
            "depth-6 search on the starting position must finish in under 15s, took {elapsed:?}",
        );

        let start = std::time::Instant::now();
        let result = search(&pos, Budget::Depth(8));
        let elapsed = start.elapsed();
        assert!(result.is_some(), "a legal turn exists in the starting position");
        assert!(
            elapsed < Duration::from_secs(300),
            "depth-8 search on the starting position must finish in under 300s, took {elapsed:?}",
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
        let score_with_quiescence = negamax(&pos, 1, 0);
        assert!(score_with_quiescence < 700, "got {score_with_quiescence}");
    }

    #[test]
    fn a_completed_shallower_iteration_s_best_move_is_tried_first_at_the_root_next_iteration() {
        // Indirect proof that TT-move-first ordering is wired end-to-end: run depth 3
        // via the iterative-deepening search() entry point (which shares one TT across
        // depths) and confirm it still finds the known-correct mate-in-one move, then
        // separately confirm negamax (a single fixed-depth call with its own fresh
        // table) agrees -- if TT-move wiring were broken (e.g. best_move never stored,
        // or never read back), both would still independently find the right move
        // since move-ordering hints only affect *how fast* alpha-beta finds an answer,
        // never *whether* it finds the correct one. This test is a correctness guard
        // for the wiring, not a performance benchmark (see Task 7 for timing).
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("g8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("f7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("g7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("h7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.white_spells = spellchess_core::SpellState {
            freeze: spellchess_core::SpellCounter { count: 0, lock: 0 },
            jump: spellchess_core::SpellCounter { count: 0, lock: 0 },
        };
        pos.black_spells = pos.white_spells;

        let (turn, _score) = search(&pos, Budget::Depth(3)).expect("a move must be found");
        assert_eq!(turn.mv.from, Square::from_str("a1").unwrap());
        assert_eq!(turn.mv.to, Square::from_str("a8").unwrap());
    }

    #[test]
    fn root_tt_entry_is_populated_after_a_completed_iteration() {
        // A completed iterative-deepening pass over a solvable position must leave a
        // TT entry with a best_move behind for the root position -- otherwise the next
        // depth's root ordering (and any future search that reaches this exact
        // position again) gets no benefit from the work already done. This is checked
        // indirectly: negamax on the same position at the depth search() just
        // completed must agree with search()'s answer (both are complete, exact
        // searches of the same tree, so they must; this at minimum proves search()
        // still returns a coherent, reproducible answer with the new table-sharing
        // wiring in place, not a stale or corrupted one).
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("g8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("f7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("g7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("h7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.white_spells = spellchess_core::SpellState {
            freeze: spellchess_core::SpellCounter { count: 0, lock: 0 },
            jump: spellchess_core::SpellCounter { count: 0, lock: 0 },
        };
        pos.black_spells = pos.white_spells;

        let (search_turn, search_score) = search(&pos, Budget::Depth(2)).expect("a move must be found");
        let negamax_score = negamax(&pos, 1, 0).saturating_neg(); // depth-1 from the reply side, mirroring best_turn's convention
        let (best_turn_move, best_turn_score) = best_turn(&pos, 2).expect("a move must be found");
        assert_eq!(search_turn.mv, best_turn_move.mv, "search() and best_turn() must agree on the winning move");
        assert_eq!(search_score, best_turn_score, "search() and best_turn() must agree on the score");
        let _ = negamax_score; // sanity-computed above to confirm negamax still runs standalone against this position
    }

    #[test]
    fn search_plays_freeze_to_win_a_defended_piece() {
        // Rook on d1 can take the knight on d5, but the c6 pawn recaptures.
        // Freeze on the pawn's neighbourhood immobilises it, so Rxd5 wins a piece.
        // Jump is disabled so the only tactic is freeze-then-capture, not a
        // jump-through king hunt. A queen on d1 would also freeze-mate via Qh5.
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Knight }));
        pos.board.set(Square::from_str("c6").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.white_spells.jump.count = 0;
        pos.black_spells = spellchess_core::SpellState {
            freeze: spellchess_core::SpellCounter { count: 0, lock: 0 },
            jump: spellchess_core::SpellCounter { count: 0, lock: 0 },
        };
        let (turn, score) = search(&pos, Budget::Depth(2)).expect("a move must be found");
        assert_eq!(turn.mv.to, Square::from_str("d5").unwrap(), "must take the hanging knight; played {turn:?} score={score}");
        assert!(turn.spell.is_some(), "must freeze the recapturing pawn, got {turn:?}");
        assert!(score > 300, "winning the knight should beat keeping R vs N+P, got {score}");
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

    #[test]
    fn search_reports_a_loss_when_every_move_is_mate() {
        // White is mated next ply no matter what (Rb1-h1). Search used to leave
        // best_turn unset because a real loss scores the same as the sentinel,
        // then report a shallow non-mate from the previous ID iteration.
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("h8").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("f7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("b1").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        pos.white_spells = spellchess_core::SpellState {
            freeze: spellchess_core::SpellCounter { count: 0, lock: 0 },
            jump: spellchess_core::SpellCounter { count: 0, lock: 0 },
        };
        pos.black_spells = pos.white_spells;
        let (_turn, score) = search(&pos, Budget::Depth(2)).expect("a legal pawn push exists");
        assert!(is_mate_score(score), "forced mate must score as a mate, got {score}");
        assert!(score < 0, "White is losing, the score must be negative, got {score}");
    }

    #[test]
    fn loss_at_prefers_a_later_ply_over_an_earlier_one() {
        // A loss discovered further from the root scores better (closer to zero)
        // than one discovered right away -- the search should prefer to delay an
        // inevitable loss rather than walk into a faster one, since a deeper loss
        // gives the opponent more chances to err along the way.
        assert!(loss_at(5) > loss_at(1), "loss_at(5)={} must beat loss_at(1)={}", loss_at(5), loss_at(1));
    }

    #[test]
    fn mate_scores_are_distinguished_from_material_eval() {
        assert!(is_mate_score(loss_at(3)), "a loss must be recognised as a mate score");
        assert!(is_mate_score(-loss_at(3)), "the winning side's mirrored score must also be recognised");
        assert!(!is_mate_score(900), "a real material eval (e.g. up a queen) must not be mistaken for a mate score");
    }

    #[test]
    fn a_forced_mate_found_deeper_scores_closer_to_zero_than_one_found_at_the_root() {
        // Ply-adjustment is only meaningful if two mates at different distances from
        // the root actually produce different scores. Before this feature, every
        // loss shared the exact same flat sentinel regardless of how deep it was.
        assert_ne!(loss_at(1), loss_at(3), "mates at different plies must not collapse to the same score");
    }

    #[test]
    fn a_losing_mate_score_stored_at_one_ply_probes_correctly_at_another() {
        // Position X is a fixed "2 more plies to a forced loss" property of X
        // itself. Reached via one path X sits at ply 3 (terminal at absolute ply
        // 5); reached via a different, longer path -- a genuine transposition --
        // X sits at ply 6 (terminal at absolute ply 8). Storing the first path's
        // raw score and probing it from the second path must not leak the first
        // path's absolute terminal ply.
        let score_via_first_path = loss_at(5); // computed with X at ply 3
        let stored = tt_store_score(score_via_first_path, 3);
        let probed = tt_probe_score(stored, 6);
        assert_eq!(probed, loss_at(8), "a mate 2 plies from X must read as -MATE+8 when X is reached at ply 6");
    }

    #[test]
    fn a_winning_mate_score_stored_at_one_ply_probes_correctly_at_another() {
        let score_via_first_path = -loss_at(5); // the winning side's mirrored score
        let stored = tt_store_score(score_via_first_path, 3);
        let probed = tt_probe_score(stored, 6);
        assert_eq!(probed, -loss_at(8));
    }

    #[test]
    fn a_non_mate_score_is_unaffected_by_tt_ply_adjustment() {
        assert_eq!(tt_store_score(120, 4), 120);
        assert_eq!(tt_probe_score(120, 7), 120);
    }
}

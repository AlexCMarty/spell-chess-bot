//! Direct computation of the captures a spell cast newly makes legal, replacing
//! the "build a hypothetical position and run a full `legal_moves` rescan" approach
//! that dominated search time (36-41us per leaf node).
//!
//! See docs/superpowers/specs/2026-09-01-spell-capture-delta-design.md.

use crate::attacks::attackers_to;
use crate::bitboard::Bitboard;
use crate::board::Board;
use crate::legal::{in_baseline, pin_ray, pins_of, SpellCast};
use crate::movegen::PieceMove;
use crate::position::{Position, SpellKind};
use crate::types::{Color, PieceKind, Square};

/// Whether the fast path could settle the question for a given cast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delta {
    /// `out` gained exactly the newly-legal captures. Authoritative.
    Complete,
    /// The fast path declined. The caller must run the rescan; `out` is untouched.
    NeedsRescan,
}

/// Captures that `cast` newly makes legal for `pos.side_to_move`, excluding
/// anything already legal in `baseline`. Appends to `out`.
///
/// Returning `NeedsRescan` is always safe: correctness never depends on this
/// function being exhaustive, only speed does.
///
/// Invariant upheld for every `NeedsRescan` return, including from future
/// (Task 3-5) code paths that speculatively push candidates into `out` before
/// concluding they can't settle the question: `out` is left exactly as it was
/// on entry. This function records `out`'s length at entry and truncates back
/// to it on every declining exit, so later additions to this function inherit
/// the invariant instead of having to re-establish it by hand.
pub fn captures_enabled_by(
    pos: &Position,
    cast: SpellCast,
    baseline: &[PieceMove],
    out: &mut Vec<PieceMove>,
) -> Delta {
    let start_len = out.len();
    let delta = match cast.kind {
        SpellKind::Jump => jump_captures(pos, cast.square, baseline, out),
        SpellKind::Freeze => Delta::NeedsRescan,
    };
    if delta == Delta::NeedsRescan {
        out.truncate(start_len);
    }
    delta
}

/// Squares `kind` attacks from `from` under `occ`. `occ` must already have live
/// jump squares subtracted, so rays run through them.
fn slider_attacks(kind: PieceKind, from: Square, occ: Bitboard) -> Bitboard {
    match kind {
        PieceKind::Bishop => crate::rays::bishop_attacks(from, occ),
        PieceKind::Rook => crate::rays::rook_attacks(from, occ),
        PieceKind::Queen => crate::rays::bishop_attacks(from, occ)
            .union(crate::rays::rook_attacks(from, occ)),
        _ => Bitboard::EMPTY,
    }
}

fn our_sliders(board: &Board, us: Color, frozen: Bitboard) -> Bitboard {
    let sliders = board
        .kind_bb(PieceKind::Bishop)
        .union(board.kind_bb(PieceKind::Rook))
        .union(board.kind_bb(PieceKind::Queen));
    board.color_bb(us).intersect(sliders).minus(frozen)
}

/// Jump only makes a square transparent to sliders (rules/40-jump.md: knights and
/// kings never benefit, and a pawn double-step is not a capture). So the captures a
/// jump newly enables for us are exactly the enemy-occupied squares our sliders
/// attack once the jumped square stops blocking.
fn jump_captures(
    pos: &Position,
    s: Square,
    baseline: &[PieceMove],
    out: &mut Vec<PieceMove>,
) -> Delta {
    let us = pos.side_to_move;
    let enemy = us.opposite();
    let Some(king_sq) = pos.board.king_square(us) else {
        return Delta::NeedsRescan;
    };

    let frozen = crate::spells::frozen_bb(pos);
    let jump_before = crate::spells::jump_bb(pos);
    let occ = pos.board.occupancy();
    let slider_occ_before = occ.minus(jump_before);
    let jump_after = jump_before.with(s);
    let slider_occ_after = occ.minus(jump_after);

    // Jump is symmetric: it can open an enemy slider onto our own king. Recompute
    // our king's check/pin context under the field rather than assuming it holds.
    let checkers_after = attackers_to(pos, king_sq, enemy, frozen, slider_occ_after);
    if checkers_after.count() > 1 {
        // Double check: only king moves are legal, and a king move is never one of
        // the slider captures below. Decline rather than reason about it.
        //
        // This also covers the one way a *non-slider* capture can be newly enabled:
        // capturing the enemy king is gated on `after_count <= before_count.max(1)`
        // in `legal_moves`, and since jump transparency only ever adds attackers,
        // that comparison can only flip from illegal to legal when our own king
        // ends up with at least two attackers under the new field.
        return Delta::NeedsRescan;
    }
    let single_checker = checkers_after.iter().next();
    let pins_after = pins_of(pos, king_sq, us, frozen, jump_after);
    let enemy_bb = pos.board.color_bb(enemy);

    for p in our_sliders(&pos.board, us, frozen).iter() {
        let kind = pos.board.get(p).expect("slider bitboard square is occupied").kind;
        let gained = slider_attacks(kind, p, slider_occ_after)
            .minus(slider_attacks(kind, p, slider_occ_before))
            .intersect(enemy_bb);
        for t in gained.iter() {
            // King capture legality is the subtlest rule in the engine (the
            // attacker_count comparison in legal_moves); do not duplicate it here.
            if pos.board.get(t).is_some_and(|q| q.kind == PieceKind::King) {
                return Delta::NeedsRescan;
            }
            if let Some(ray) = pin_ray(&pins_after, p) {
                if !ray.contains(t) {
                    continue;
                }
                // A pinned piece capturing *onto a live jump square* is the one
                // case `legal_moves` itself resolves by clone-and-rescan (taking a
                // jumped pinner is legal, taking a jumped between-piece is not).
                // `t` is never `s` -- a ray always includes its own first blocker,
                // so `s` can't be newly gained -- so this only fires when some
                // *other* jump field is already live. Don't duplicate that rule.
                if jump_after.contains(t) {
                    return Delta::NeedsRescan;
                }
            }
            // In check, a slider capture only helps if it takes the checker;
            // interposing is not a capture.
            if let Some(checker) = single_checker {
                if t != checker {
                    continue;
                }
            }
            let mv = PieceMove::quiet(p, t);
            if in_baseline(baseline, mv) {
                continue;
            }
            out.push(mv);
        }
    }
    Delta::Complete
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legal::legal_moves;
    use crate::position::SpellKind;
    use crate::types::Square;

    #[test]
    fn declining_leaves_the_output_buffer_untouched() {
        let pos = Position::starting();
        let baseline = legal_moves(&pos);
        let cast = SpellCast { kind: SpellKind::Freeze, square: Square::from_str("d4").unwrap() };
        let mut out = vec![baseline[0]];
        let before = out.clone();
        if captures_enabled_by(&pos, cast, &baseline, &mut out) == Delta::NeedsRescan {
            assert_eq!(out, before, "a declining call must not touch `out`");
        }
    }

    /// The one geometry the ray delta cannot settle on its own: a slider that the
    /// jump *itself* pins, newly reaching an enemy piece that a **second**, already
    /// live jump field keeps transparent. `legal_moves` resolves that by
    /// clone-and-rescan (capture the jumped pinner: legal; capture a jumped
    /// between-piece: not), so `jump_captures` must decline rather than emit.
    ///
    /// Here white Re2 is unpinned until jump@e3 makes the white pawn transparent;
    /// that same field opens the file so that Bxe4 (Black's jumped knight) would
    /// leave Ke1 hanging to Re5 straight through the two transparent squares.
    #[test]
    fn a_pinned_slider_capturing_onto_a_live_jump_square_declines() {
        use crate::board::Board;
        use crate::position::SpellField;
        use crate::types::{Color, Piece, PieceKind};

        let sq = |s: &str| Square::from_str(s).unwrap();
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(sq("e1"), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(sq("e2"), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(sq("e3"), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        pos.board.set(sq("e4"), Some(Piece { color: Color::Black, kind: PieceKind::Knight }));
        pos.board.set(sq("e5"), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        pos.board.set(sq("a8"), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.fields.push(SpellField {
            square: sq("e4"),
            owner: Color::Black,
            kind: SpellKind::Jump,
            expires_after_ply: pos.ply + 1,
        });

        let cast = SpellCast { kind: SpellKind::Jump, square: sq("e3") };
        let baseline = legal_moves(&pos);
        let rxe4 = PieceMove::quiet(sq("e2"), sq("e4"));
        assert!(!baseline.contains(&rxe4));

        // Ground truth: the rescan rejects Rxe4 -- the rook lands on a square that
        // stays transparent, so Re5 still sees Ke1 through e3 and e4.
        let hypothetical = crate::legal::position_with_field(&pos, cast);
        assert!(!legal_moves(&hypothetical).contains(&rxe4));

        let mut out = Vec::new();
        assert_eq!(captures_enabled_by(&pos, cast, &baseline, &mut out), Delta::NeedsRescan);
        assert!(out.is_empty(), "a declining call must not touch `out`");
    }
}

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
        SpellKind::Freeze => freeze_captures(pos, cast.square, baseline, out),
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

/// Pseudo-legal capture destinations for the piece standing on `from`, ignoring
/// pins and check -- the caller filters those. `slider_occ` already has live jump
/// squares subtracted.
fn piece_capture_targets(pos: &Position, from: Square, slider_occ: Bitboard, enemy_bb: Bitboard) -> Bitboard {
    let Some(piece) = pos.board.get(from) else { return Bitboard::EMPTY };
    let idx = from.0 as usize;
    let raw = match piece.kind {
        PieceKind::Knight => crate::rays::KNIGHT_ATTACKS[idx],
        PieceKind::King => crate::rays::KING_ATTACKS[idx],
        PieceKind::Pawn => crate::rays::PAWN_ATTACKS[piece.color.index()][idx],
        kind => slider_attacks(kind, from, slider_occ),
    };
    raw.intersect(enemy_bb)
}

/// Freeze never changes reachability, only control (rules/30-freeze.md: frozen
/// pieces "exert no control" but "still block sliding pieces"). Occupancy is
/// untouched, so `pseudo_legal_moves` can only shrink; every newly *legal* move was
/// already pseudo-legal and was rejected by one of `legal_moves`' filters. Freeze
/// can therefore only flip one of these:
///
/// 1. the check filters (`checker_count >= 2`, `evasion_allows`) -- declined below;
/// 2. the pin filter -- the released pins this function computes. Inside that filter
///    sits a clone-and-rescan arm for a pinned piece capturing onto a live jump
///    square, which freeze can flip without touching the pin at all -- declined below;
/// 3. `king_dest_safe` for an enemy piece standing beside our king -- computed below;
/// 4. `after_count <= before_count.max(1)` for capturing the enemy king -- declined
///    below.
///
/// That list is the spec, and nothing links it mechanically to `legal_moves`: anyone
/// adding a filter there must revisit it here.
fn freeze_captures(
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
    // See the sort just before the final `Delta::Complete`: this call's own slice of
    // `out` has to come out in `pseudo_legal_moves` order, and it is built in two
    // passes that do not interleave.
    let out_start = out.len();

    let frozen_before = crate::spells::frozen_bb(pos);
    let zone = crate::spells::FREEZE_ZONE[s.0 as usize];
    let frozen_after = frozen_before.union(zone);
    let enemy_bb = pos.board.color_bb(enemy);
    let newly_frozen_them = frozen_after.minus(frozen_before).intersect(enemy_bb);

    // Freezing only our own pieces (or nothing) strictly *removes* enemy-free
    // control from nobody: no enemy piece loses control, so no move of ours can
    // become legal. `move_survives_own_freeze` in legal.rs handles the losses.
    if newly_frozen_them.is_empty() {
        return Delta::Complete;
    }

    // En passant is a capture and interacts with freeze in ways this fast path
    // does not model; the rescan already special-cases it.
    if pos.en_passant.is_some() {
        return Delta::NeedsRescan;
    }

    let jump = crate::spells::jump_bb(pos);
    let occ = pos.board.occupancy();
    let slider_occ = occ.minus(jump);

    let checkers_before = crate::attacks::attackers_to(pos, king_sq, enemy, frozen_before, slider_occ);
    if !checkers_before.is_empty() {
        // Check dispelled (or still live): the newly legal set is essentially the
        // whole position, which is not a cheap delta. This is what the escape
        // hatch exists for.
        //
        // Testing `checkers_before` alone is exhaustive; there is deliberately no
        // `checkers_after` term. A freeze can never *add* a checker: `attackers_to`
        // is monotonically decreasing in `frozen` (attacks.rs walks `unfrozen()`),
        // `frozen_after` is a superset of `frozen_before`, and a freeze leaves
        // occupancy -- and therefore `slider_occ` -- untouched. So `checkers_after`
        // is always a subset of `checkers_before`, and the disjunct it would
        // contribute can never decide this branch. It used to be spelled out here and
        // cost a full `attackers_to` per freeze target per leaf on the hottest path.
        // Anyone who makes freeze touch occupancy or jump fields must revisit this.
        return Delta::NeedsRescan;
    }

    // Mechanism 4. Capturing the enemy king is gated on
    // `after_count <= before_count.max(1)`, and freezing enemy pieces only shrinks
    // `after_count` -- so a king capture that was illegal can become legal. That
    // rule is the subtlest in the engine; don't duplicate it, just decline whenever
    // we have any pseudo-legal shot at the enemy king.
    //
    // This guard ("guard 3" in the Task 5 review) is provably dominated: a reviewer
    // showed every case it declines is already caught downstream, by the
    // pinned-piece-captures-onto-a-live-jump-square guard a few lines below plus the
    // king-takes-king decline in the mechanism-3 king_targets loop further down. No
    // mutation of this block is killed by any test, and a ~72k-position differential
    // sweep against the oracle with this guard and the ray-growth guard below both
    // deleted found zero disagreements. It is kept anyway, because it is the stated
    // precondition for mechanism 2's emission comment ("never reaches the enemy king
    // (the mechanism-4 guard above already declined)"): delete this and mechanism 2
    // would emit enemy-king captures whose legality rests on reasoning no longer
    // encoded anywhere in the code, true by luck rather than by construction.
    if let Some(their_king) = pos.board.king_square(enemy) {
        if !crate::attacks::attackers_to(pos, their_king, us, frozen_after, slider_occ).is_empty() {
            return Delta::NeedsRescan;
        }
    }

    let pins_before = pins_of(pos, king_sq, us, frozen_before, jump);
    let pins_after = pins_of(pos, king_sq, us, frozen_after, jump);

    // A piece that stays pinned, with the very same ray, can still gain a capture --
    // so neither `released` nor a ray comparison is enough on its own.
    let jump_enemy = jump.intersect(enemy_bb);
    for p in pins_after.pinned.iter() {
        // A pin ray can *grow* rather than vanish: a frozen pinner sitting on a live
        // jump square stops pinning but stays transparent, so `pins_of` walks on to a
        // further slider. The blocker is still pinned -- so it never reaches
        // `released` -- yet its ray, and with it its legal captures, just got longer.
        // (This arm also covers a piece pinned only *after* the cast, whose
        // before-ray is empty.)
        //
        // This guard ("guard 4" in the Task 5 review) is provably dominated by the
        // pinned-piece-captures-onto-a-live-jump-square guard immediately below: a
        // reviewer showed every case where the ray grows is already caught there. No
        // mutation of this `if` is killed by any test, and the same ~72k-position
        // differential sweep that cleared guard 3 above found zero disagreements with
        // both this guard and guard 3 deleted. Kept deliberately alongside guard 3 (see
        // its comment) rather than deleted: the sweep is a soundness spot-check, not a
        // proof, and the guard costs nothing to keep -- `pins_after.rays` is already
        // computed for the jump-square check that follows.
        if pins_before.rays[p.0 as usize] != pins_after.rays[p.0 as usize] {
            return Delta::NeedsRescan;
        }
        // A pinned piece capturing *onto a live jump square* is the one case
        // `legal_moves` resolves by clone-and-rescan (legal.rs: taking a jumped
        // pinner can be legal, taking a jumped between-piece is not). That rescan
        // runs on the *hypothetical* position, so freezing a slider elsewhere on the
        // ray can flip it to legal while leaving the pin and its ray untouched.
        // `jump_captures` declines the mirror-image case; do the same rather than
        // duplicate the rule. `jump_enemy` is empty in the overwhelmingly common
        // no-live-jump-field case, so this costs nothing.
        if !pins_after.rays[p.0 as usize].intersect(jump_enemy).is_empty() {
            return Delta::NeedsRescan;
        }
    }

    // Mechanism 2, the one this function actually computes.
    let released = pins_before.pinned.minus(pins_after.pinned);
    for p in released.iter() {
        // Freeze hits our own pieces too (rules/30-freeze.md: "every piece in the
        // zone regardless of owner"), and a field the opponent laid last ply may
        // still be pinning ours down from outside the zone we are casting. Either
        // way a frozen piece has zero legal moves and contributes no captures.
        if frozen_after.contains(p) {
            continue;
        }
        let piece = pos.board.get(p).expect("pinned square is occupied");
        // Pawn captures carry promotion and en-passant variants; not worth
        // modelling here for a case this rare.
        if piece.kind == PieceKind::Pawn {
            return Delta::NeedsRescan;
        }
        // `p` is never our king (a king is never its own pin blocker) and never
        // reaches the enemy king (the mechanism-4 guard above already declined),
        // so every target here is an ordinary piece and, with no check and no pin
        // left, an unconditionally legal capture.
        for t in piece_capture_targets(pos, p, slider_occ, enemy_bb).iter() {
            let mv = PieceMove::quiet(p, t);
            if in_baseline(baseline, mv) {
                continue;
            }
            out.push(mv);
        }
    }

    // Mechanism 3, the other one this function computes. A frozen piece exerts no
    // control (rules/30-freeze.md), so freezing the last defender of an enemy piece
    // standing beside our king lets the king take it -- no pin is involved, so the
    // released loop above cannot see it.
    //
    // `legal_moves` filters a king move with exactly one test, `king_dest_safe`, and
    // a king capture of an adjacent enemy piece is pseudo-legal whenever the king is
    // unfrozen -- so evaluating that one predicate against the hypothetical (which
    // carries the new freeze field, and which `king_dest_safe` itself probes by
    // clearing both `king_sq` and the target) is the whole rule, not an approximation
    // of it.
    //
    // Skipped when our king is frozen *after* the cast. That covers two distinct
    // traps, and `zone.contains(king_sq)` would only catch the first: our king caught
    // in our own zone (freeze hits every piece in the 3x3 regardless of owner), and
    // our king already frozen by an older field the opponent laid anywhere on the
    // board. A frozen king is frozen like any other piece, even in check
    // (`sacredRoyal` off is canonical), and `pseudo_legal_moves` walks
    // `own.minus(frozen)` -- so in both cases it has no moves at all to contribute.
    let king_targets = crate::rays::KING_ATTACKS[king_sq.0 as usize].intersect(enemy_bb);
    if !frozen_after.contains(king_sq) && !king_targets.is_empty() {
        let cast = SpellCast { kind: SpellKind::Freeze, square: s };
        let hypo = crate::legal::position_with_field(pos, cast);
        for t in king_targets.iter() {
            // Capturing the enemy king runs through `after_count <= before_count.max(1)`
            // instead of `king_dest_safe`; don't duplicate that rule, decline. This is
            // dead today -- an unfrozen king of ours adjacent to theirs is itself an
            // attacker of their king, so the mechanism-4 guard above already declined
            // -- but it is what keeps this loop honest if that guard is ever narrowed.
            if pos.board.get(t).is_some_and(|q| q.kind == PieceKind::King) {
                return Delta::NeedsRescan;
            }
            let mv = PieceMove::quiet(king_sq, t);
            if in_baseline(baseline, mv) {
                continue;
            }
            if crate::legal::king_dest_safe(&hypo, king_sq, t, enemy) {
                out.push(mv);
            }
        }
    }

    // The delta has to reproduce the rescan's *sequence*, not just its set:
    // `generate_quiescence_from` pushes these in emission order and neither
    // `dedup_captures` nor the quiescence loop sorts, so a different order is a
    // different beta-cutoff order and a different qnode count.
    //
    // `pseudo_legal_moves` walks `own.minus(frozen)` in ascending square order and
    // emits each piece's destinations ascending too (bitboard iteration; castles come
    // first but are never captures), and `legal_moves` only filters, so the rescan is
    // strictly ascending by `(from, to)`. The two mechanisms above are not: mechanism
    // 2 emits ascending over `released`, then mechanism 3 appends the king's captures
    // last regardless of where the king sits. Sorting this call's own slice -- never
    // anything a caller already had in `out` -- restores the rescan's order.
    //
    // Sorting unconditionally, rather than only when both blocks are non-empty (each is
    // already ascending on its own, so that is the only case that can be out of order),
    // was measured: guarding it changes depth-6 wall time by nothing. The ~0.05s this
    // costs is the monomorphised sort's code size in a hot function, not the call.
    //
    // The key is total here: ties on `(from, to)` would need promotions or en
    // passant, and both paths into this function decline before emitting either (the
    // pawn arm of the released loop, and the `pos.en_passant.is_some()` guard). The
    // promotion component is carried anyway so the key stays faithful if that ever
    // changes -- `Promotion::ALL` is declared in `as u8` order, which is exactly the
    // order `add_pawn_move` emits.
    out[out_start..]
        .sort_unstable_by_key(|m| (m.from.0, m.to.0, m.promotion.map_or(0u8, |p| p as u8)));

    Delta::Complete
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legal::legal_moves;
    use crate::position::SpellKind;
    use crate::types::Square;

    fn sq(s: &str) -> Square {
        Square::from_str(s).unwrap()
    }

    fn put(pos: &mut Position, s: &str, color: Color, kind: PieceKind) {
        pos.board.set(sq(s), Some(crate::types::Piece { color, kind }));
    }

    fn empty_board() -> Position {
        Position { board: crate::board::Board::empty(), ..Position::starting() }
    }

    fn add_field(pos: &mut Position, s: &str, owner: Color, kind: SpellKind) {
        pos.fields.push(crate::position::SpellField {
            square: sq(s),
            owner,
            kind,
            expires_after_ply: pos.ply + 1,
        });
    }

    /// Exactly what `generate_quiescence_from`'s rescan branch computes; the same
    /// body as `rescan_oracle` in tests/spell_delta_soundness.rs.
    fn rescan_oracle(pos: &Position, cast: SpellCast, baseline: &[PieceMove]) -> Vec<PieceMove> {
        legal_moves(&crate::legal::position_with_field(pos, cast))
            .into_iter()
            .filter(|mv| {
                let is_cap = pos.board.get(mv.to).is_some() || mv.is_en_passant;
                is_cap && !baseline.contains(mv)
            })
            .collect()
    }

    fn key(mv: &PieceMove) -> (u8, u8, u8, bool, bool) {
        (mv.from.0, mv.to.0, mv.promotion.map(|p| p as u8).unwrap_or(255), mv.is_en_passant, mv.is_castle)
    }

    /// Assert the delta either declines or agrees exactly with the rescan.
    fn assert_delta_sound(pos: &Position, cast: SpellCast) -> Delta {
        let baseline = legal_moves(pos);
        let mut fast = Vec::new();
        let delta = captures_enabled_by(pos, cast, &baseline, &mut fast);
        if delta == Delta::Complete {
            let mut got: Vec<_> = fast.iter().map(key).collect();
            let mut want: Vec<_> = rescan_oracle(pos, cast, &baseline).iter().map(key).collect();
            got.sort();
            want.sort();
            assert_eq!(got, want, "delta disagreed with the rescan for {cast:?}");
        } else {
            assert!(fast.is_empty(), "a declining call must not touch `out`");
        }
        delta
    }

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

    #[test]
    fn freezing_a_pinner_frees_the_pinned_piece_to_capture() {
        // Black Rd8 pins White Rd4 to Kd1 down the d-file. Rd4 cannot take the
        // undefended Be4 while pinned. freeze@c8 covers d8, killing the pin, so
        // Rd4xe4 becomes legal -- a capture only the freeze enables.
        let mut pos = Position { board: crate::board::Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("d1").unwrap(), Some(crate::types::Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d4").unwrap(), Some(crate::types::Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e4").unwrap(), Some(crate::types::Piece { color: Color::Black, kind: PieceKind::Bishop }));
        pos.board.set(Square::from_str("d8").unwrap(), Some(crate::types::Piece { color: Color::Black, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("h8").unwrap(), Some(crate::types::Piece { color: Color::Black, kind: PieceKind::King }));

        let baseline = crate::legal::legal_moves(&pos);
        let d4e4 = PieceMove::quiet(Square::from_str("d4").unwrap(), Square::from_str("e4").unwrap());
        assert!(!baseline.contains(&d4e4), "Rd4xe4 must be pinned-illegal without the spell");

        let cast = SpellCast { kind: SpellKind::Freeze, square: Square::from_str("c8").unwrap() };
        let mut out = Vec::new();
        assert_eq!(captures_enabled_by(&pos, cast, &baseline, &mut out), Delta::Complete);
        assert!(out.contains(&d4e4), "freezing the pinner must enable Rd4xe4, got {out:?}");
    }

    #[test]
    fn freezing_the_last_defender_lets_the_king_capture() {
        // Black Nd2 sits next to White Ke1, defended only by Ra2. Kxd2 is illegal.
        // freeze@a3 covers a2, so the knight is undefended and Kxd2 becomes legal.
        let mut pos = Position { board: crate::board::Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(crate::types::Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d2").unwrap(), Some(crate::types::Piece { color: Color::Black, kind: PieceKind::Knight }));
        pos.board.set(Square::from_str("a2").unwrap(), Some(crate::types::Piece { color: Color::Black, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("h8").unwrap(), Some(crate::types::Piece { color: Color::Black, kind: PieceKind::King }));

        let baseline = crate::legal::legal_moves(&pos);
        let kxd2 = PieceMove::quiet(Square::from_str("e1").unwrap(), Square::from_str("d2").unwrap());
        assert!(!baseline.contains(&kxd2), "Kxd2 must be illegal while Ra2 defends d2");

        let cast = SpellCast { kind: SpellKind::Freeze, square: Square::from_str("a3").unwrap() };
        let mut out = Vec::new();
        assert_eq!(captures_enabled_by(&pos, cast, &baseline, &mut out), Delta::Complete);
        assert!(out.contains(&kxd2), "freezing the defender must enable Kxd2, got {out:?}");
    }

    /// The own-freeze trap, half one: our king inside the zone we are casting. Freeze
    /// hits every piece in the 3x3 regardless of owner (rules/30-freeze.md), and a
    /// frozen king is frozen like any other piece -- `pseudo_legal_moves` walks
    /// `own.minus(frozen)`, so it has *no* moves. The removed-defender mechanism must
    /// therefore contribute nothing, even though the defender really is silenced.
    #[test]
    fn a_king_caught_in_our_own_freeze_zone_captures_nothing() {
        // Nb2 sits beside Ka1, guarded only by Rb3. freeze@a2 silences Rb3 -- and
        // freezes our own king along with it, so Kxb2 stays illegal.
        let mut pos = empty_board();
        put(&mut pos, "a1", Color::White, PieceKind::King);
        put(&mut pos, "d4", Color::White, PieceKind::Rook);
        put(&mut pos, "b2", Color::Black, PieceKind::Knight);
        put(&mut pos, "b3", Color::Black, PieceKind::Rook);
        put(&mut pos, "h8", Color::Black, PieceKind::King);

        let baseline = legal_moves(&pos);
        let kxb2 = PieceMove::quiet(sq("a1"), sq("b2"));
        assert!(!baseline.contains(&kxb2), "fixture is wrong: Rb3 must guard b2");
        // The geometry really is live: a cast that silences Rb3 while sparing our
        // king does legalise Kxb2, so this fixture is not passing by accident.
        let spares_king = SpellCast { kind: SpellKind::Freeze, square: sq("b4") };
        assert!(
            rescan_oracle(&pos, spares_king, &baseline).contains(&kxb2),
            "fixture is wrong: freeze@b4 must enable Kxb2",
        );
        assert_delta_sound(&pos, spares_king);

        // ...but freeze@a2 covers a1 as well, so the same silencing buys nothing.
        let traps_king = SpellCast { kind: SpellKind::Freeze, square: sq("a2") };
        assert!(
            rescan_oracle(&pos, traps_king, &baseline).is_empty(),
            "fixture is wrong: a frozen king has no moves",
        );
        assert_delta_sound(&pos, traps_king);
    }

    /// The own-freeze trap, half two -- the half `zone.contains(king_sq)` cannot see.
    /// Our king can already be frozen by a field the *opponent* laid last ply, far
    /// outside the zone we are casting. It still has zero legal moves, so the
    /// removed-defender mechanism must still contribute nothing. This is Task 4's
    /// bug 1 one level up: the zone is not the predicate, `frozen_after` is.
    #[test]
    fn a_king_frozen_by_an_older_field_captures_nothing() {
        // Nd2 sits beside Ke1, guarded only by Rb2. freeze@b3 silences Rb2 and comes
        // nowhere near e1 -- but Black froze e1 last ply, so Kxd2 remains illegal.
        let mut pos = empty_board();
        put(&mut pos, "e1", Color::White, PieceKind::King);
        put(&mut pos, "a1", Color::White, PieceKind::Rook);
        put(&mut pos, "d2", Color::Black, PieceKind::Knight);
        put(&mut pos, "b2", Color::Black, PieceKind::Rook);
        put(&mut pos, "h8", Color::Black, PieceKind::King);

        let cast = SpellCast { kind: SpellKind::Freeze, square: sq("b3") };
        let kxd2 = PieceMove::quiet(sq("e1"), sq("d2"));
        assert!(!crate::spells::FREEZE_ZONE[sq("b3").0 as usize].contains(sq("e1")));

        // Without the opponent's field this is a textbook mechanism-3 win.
        let free_baseline = legal_moves(&pos);
        assert!(!free_baseline.contains(&kxd2), "fixture is wrong: Rb2 must guard d2");
        assert!(
            rescan_oracle(&pos, cast, &free_baseline).contains(&kxd2),
            "fixture is wrong: freeze@b3 must enable Kxd2 for an unfrozen king",
        );
        assert_delta_sound(&pos, cast);

        // Black froze e1 on their turn; the field is still live on ours.
        add_field(&mut pos, "e1", Color::Black, SpellKind::Freeze);
        let baseline = legal_moves(&pos);
        assert!(
            !baseline.iter().any(|mv| mv.from == sq("e1")),
            "fixture is wrong: our king must be frozen solid",
        );
        assert!(
            !rescan_oracle(&pos, cast, &baseline).iter().any(|mv| mv.from == sq("e1")),
            "fixture is wrong: a frozen king has no moves",
        );
        assert_delta_sound(&pos, cast);
    }

    /// `king_dest_safe` is the wrong rule for *capturing the enemy king*, which
    /// `legal_moves` decides with `after_count <= before_count.max(1)` instead -- a
    /// king may legally walk onto a defended square to take a king. So the
    /// removed-defender loop must never answer for an enemy-king target; it declines.
    ///
    /// The geometry only exists because the enemy king is frozen: two adjacent
    /// *unfrozen* kings are in mutual check, and the check guard would decline first.
    #[test]
    fn freeze_that_legalises_taking_a_frozen_enemy_king_declines() {
        // We froze Black's king on c3 last turn, so Kd2 gives no check. Ke1xd2 is
        // illegal while both Ra2 and Rd8 cover d2 (after_count 2 > 1); freeze@a1
        // silences Ra2 and makes it legal. `king_dest_safe` would say the opposite,
        // since Rd8 still covers d2.
        let mut pos = empty_board();
        put(&mut pos, "e1", Color::White, PieceKind::King);
        put(&mut pos, "d2", Color::Black, PieceKind::King);
        put(&mut pos, "a2", Color::Black, PieceKind::Rook);
        put(&mut pos, "d8", Color::Black, PieceKind::Rook);
        add_field(&mut pos, "c3", Color::White, SpellKind::Freeze);

        let baseline = legal_moves(&pos);
        let kxd2 = PieceMove::quiet(sq("e1"), sq("d2"));
        assert!(!baseline.contains(&kxd2), "fixture is wrong: Ke1xd2 must start illegal");
        let cast = SpellCast { kind: SpellKind::Freeze, square: sq("a1") };
        assert!(
            rescan_oracle(&pos, cast, &baseline).contains(&kxd2),
            "fixture is wrong: freeze@a1 must enable Ke1xd2",
        );
        // ...and `king_dest_safe` disagrees, which is the whole point: d2 is still
        // covered by Rd8 in the hypothetical, so emitting on that predicate would
        // *drop* a legal capture from a supposedly authoritative answer.
        let hypo = crate::legal::position_with_field(&pos, cast);
        assert!(!crate::legal::king_dest_safe(&hypo, sq("e1"), sq("d2"), Color::Black));
        assert_delta_sound(&pos, cast);
    }

    /// Mechanism 3 (Task 5's): freezing the last defender of a piece standing next
    /// to our king lets the king take it. That is a capture the freeze enables with
    /// no pin involved, so the released-pin delta must decline rather than claim
    /// `Complete` on an answer that misses it.
    #[test]
    fn freezing_the_last_defender_of_a_piece_next_to_our_king_must_not_claim_complete() {
        // Black Nd2 sits beside Ke1, guarded only by Rd8 down the d-file. Kxd2 is
        // illegal now; freeze@d8 removes the guard and makes it legal.
        let mut pos = empty_board();
        put(&mut pos, "e1", Color::White, PieceKind::King);
        put(&mut pos, "d2", Color::Black, PieceKind::Knight);
        put(&mut pos, "d8", Color::Black, PieceKind::Rook);
        put(&mut pos, "h8", Color::Black, PieceKind::King);

        let baseline = legal_moves(&pos);
        let kxd2 = PieceMove::quiet(sq("e1"), sq("d2"));
        assert!(!baseline.contains(&kxd2), "Kxd2 must be guard-illegal without the spell");
        let cast = SpellCast { kind: SpellKind::Freeze, square: sq("d8") };
        assert!(
            rescan_oracle(&pos, cast, &baseline).contains(&kxd2),
            "fixture is wrong: freeze@d8 must enable Kxd2",
        );
        assert_delta_sound(&pos, cast);
    }

    /// Freeze can newly enable *capturing the enemy king*: the legality test is
    /// `after_count <= before_count.max(1)` and freezing enemy pieces shrinks
    /// `after_count`. Here two jump-transparent black rooks share the e-file behind
    /// the white bishop, so Bxd8 exposes Ke1 to both at once -- unless one is frozen.
    #[test]
    fn freezing_an_attacker_that_makes_an_enemy_king_capture_legal_must_not_claim_complete() {
        let mut pos = empty_board();
        put(&mut pos, "e1", Color::White, PieceKind::King);
        put(&mut pos, "e4", Color::White, PieceKind::Bishop);
        put(&mut pos, "d5", Color::Black, PieceKind::King);
        put(&mut pos, "e6", Color::Black, PieceKind::Rook);
        put(&mut pos, "e7", Color::Black, PieceKind::Rook);
        put(&mut pos, "a1", Color::White, PieceKind::Rook);
        add_field(&mut pos, "e6", Color::Black, SpellKind::Jump);
        add_field(&mut pos, "e7", Color::Black, SpellKind::Jump);

        let baseline = legal_moves(&pos);
        let bxd5 = PieceMove::quiet(sq("e4"), sq("d5"));
        assert!(!baseline.contains(&bxd5), "fixture is wrong: Bxd5 must start illegal");
        // freeze@e8 covers e7 but *not* e6, so the e6 pin on Be4 survives and the
        // released-pin delta has nothing to say -- yet Bxd5 becomes legal.
        let cast = SpellCast { kind: SpellKind::Freeze, square: sq("e8") };
        assert!(
            rescan_oracle(&pos, cast, &baseline).contains(&bxd5),
            "fixture is wrong: freeze@e8 must enable Bxd5",
        );
        assert_delta_sound(&pos, cast);
    }

    /// A pin ray can *grow* instead of vanishing: freezing a pinner that stands on a
    /// live jump square makes it a non-pinner but leaves it transparent, so the walk
    /// runs on to a further enemy slider. The blocker stays pinned (so it is not in
    /// `released`) yet gains new legal destinations, including a capture.
    #[test]
    fn a_pin_ray_that_grows_under_freeze_must_not_claim_complete() {
        // Rd5 pins Rd3 to Kd1 with a ray stopping at d5. freeze@d6 covers d5 only:
        // the frozen rook stops pinning but stays jump-transparent, so the walk runs
        // on to Qd8 and the pin ray now reaches d8. Rd3xd8 is newly legal.
        let mut pos = empty_board();
        put(&mut pos, "d1", Color::White, PieceKind::King);
        put(&mut pos, "d3", Color::White, PieceKind::Rook);
        put(&mut pos, "d5", Color::Black, PieceKind::Rook);
        put(&mut pos, "d8", Color::Black, PieceKind::Queen);
        put(&mut pos, "h8", Color::Black, PieceKind::King);
        add_field(&mut pos, "d5", Color::Black, SpellKind::Jump);

        let baseline = legal_moves(&pos);
        let rxd8 = PieceMove::quiet(sq("d3"), sq("d8"));
        assert!(!baseline.contains(&rxd8), "fixture is wrong: Rxd8 must start illegal");
        let cast = SpellCast { kind: SpellKind::Freeze, square: sq("d6") };
        assert!(
            rescan_oracle(&pos, cast, &baseline).contains(&rxd8),
            "fixture is wrong: freeze@d6 must enable Rxd8",
        );
        assert_delta_sound(&pos, cast);
    }

    /// A piece released from a pin can already be frozen by a field the *opponent*
    /// laid last ply, well outside the zone we are about to cast. It has no legal
    /// moves at all, so it must not contribute captures.
    #[test]
    fn a_released_piece_already_frozen_by_an_older_field_contributes_nothing() {
        let mut pos = empty_board();
        put(&mut pos, "d1", Color::White, PieceKind::King);
        put(&mut pos, "d4", Color::White, PieceKind::Rook);
        put(&mut pos, "e4", Color::Black, PieceKind::Bishop);
        put(&mut pos, "d8", Color::Black, PieceKind::Rook);
        put(&mut pos, "h8", Color::Black, PieceKind::King);
        // Black froze d4 on their turn; the field is still live on ours.
        add_field(&mut pos, "d4", Color::Black, SpellKind::Freeze);

        let baseline = legal_moves(&pos);
        let cast = SpellCast { kind: SpellKind::Freeze, square: sq("c8") };
        assert!(
            rescan_oracle(&pos, cast, &baseline).is_empty(),
            "fixture is wrong: a frozen rook has no moves",
        );
        assert_delta_sound(&pos, cast);
    }

    /// The pin can survive the cast *completely unchanged* and still gain a capture.
    /// `legal_moves` resolves "a pinned piece captures onto a live jump square" by
    /// clone-and-rescan (legal.rs), and that rescan runs on the **hypothetical**
    /// position -- so freezing a slider somewhere else on the ray can legalise it.
    /// `released` is empty and the ray-change guard passes, so nothing else here
    /// would catch it.
    #[test]
    fn a_pinned_piece_capturing_onto_a_live_jump_square_declines_under_freeze() {
        // Rd5 pins Rd3 to Kd1 and sits on a live jump field, so Rxd5 lands on a
        // square that stays transparent and leaves Qd8 shooting at d1 -- illegal.
        // freeze@d7 covers d8 but not d5: the pin and its ray are untouched, yet
        // silencing the queen makes exactly that capture legal.
        let mut pos = empty_board();
        put(&mut pos, "d1", Color::White, PieceKind::King);
        put(&mut pos, "d3", Color::White, PieceKind::Rook);
        put(&mut pos, "d5", Color::Black, PieceKind::Rook);
        put(&mut pos, "d8", Color::Black, PieceKind::Queen);
        put(&mut pos, "h8", Color::Black, PieceKind::King);
        add_field(&mut pos, "d5", Color::Black, SpellKind::Jump);

        let baseline = legal_moves(&pos);
        let rxd5 = PieceMove::quiet(sq("d3"), sq("d5"));
        assert!(!baseline.contains(&rxd5), "fixture is wrong: Rxd5 must start illegal");
        let cast = SpellCast { kind: SpellKind::Freeze, square: sq("d7") };
        assert!(
            rescan_oracle(&pos, cast, &baseline).contains(&rxd5),
            "fixture is wrong: freeze@d7 must enable Rxd5",
        );
        assert_delta_sound(&pos, cast);
    }

    /// A released pin can hand a *pawn* a capture, and on the last rank that capture
    /// carries four promotion variants that `PieceMove::quiet` cannot express. The
    /// geometry needs the king on the far side of the pawn -- a pinner beyond a
    /// rank-7 white pawn would have to stand on rank 8, adjacent to it, so the same
    /// cast would always freeze the pawn too.
    #[test]
    fn a_released_pawn_that_captures_into_promotion_declines() {
        let mut pos = empty_board();
        put(&mut pos, "e8", Color::White, PieceKind::King);
        put(&mut pos, "e7", Color::White, PieceKind::Pawn);
        put(&mut pos, "e6", Color::Black, PieceKind::Rook);
        put(&mut pos, "d8", Color::Black, PieceKind::Knight);
        put(&mut pos, "a1", Color::Black, PieceKind::King);

        let baseline = legal_moves(&pos);
        let quiet_exd8 = PieceMove::quiet(sq("e7"), sq("d8"));
        assert!(!baseline.contains(&quiet_exd8), "fixture is wrong: the pawn starts pinned");
        // freeze@d5 covers e6 but not e7: the pin dies, the pawn does not.
        let cast = SpellCast { kind: SpellKind::Freeze, square: sq("d5") };
        let enabled = rescan_oracle(&pos, cast, &baseline);
        assert!(
            enabled.iter().any(|mv| mv.from == sq("e7") && mv.to == sq("d8") && mv.promotion.is_some()),
            "fixture is wrong: freeze@d5 must enable exd8=Q, got {enabled:?}",
        );
        assert!(!enabled.contains(&quiet_exd8), "a promotion capture is never promotion-less");
        assert_delta_sound(&pos, cast);
    }

    /// The classic en-passant pin: exd6 would clear *two* pawns off rank 5 at once
    /// and expose Kh5 to Ra5, which is not a pin `pins_of` can see. Freezing the rook
    /// makes it legal, so the released-pin delta would miss it entirely.
    #[test]
    fn an_en_passant_capture_unlocked_by_freezing_the_x_ray_rook_declines() {
        let mut pos = empty_board();
        put(&mut pos, "h5", Color::White, PieceKind::King);
        put(&mut pos, "e5", Color::White, PieceKind::Pawn);
        put(&mut pos, "d5", Color::Black, PieceKind::Pawn);
        put(&mut pos, "a5", Color::Black, PieceKind::Rook);
        put(&mut pos, "h8", Color::Black, PieceKind::King);
        pos.en_passant = Some(sq("d6"));

        let baseline = legal_moves(&pos);
        assert!(!baseline.iter().any(|mv| mv.is_en_passant), "fixture is wrong: exd6 must start illegal");
        let cast = SpellCast { kind: SpellKind::Freeze, square: sq("a5") };
        assert!(
            rescan_oracle(&pos, cast, &baseline).iter().any(|mv| mv.is_en_passant),
            "fixture is wrong: freeze@a5 must enable exd6 e.p.",
        );
        assert_delta_sound(&pos, cast);
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

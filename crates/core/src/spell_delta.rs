//! Direct computation of the captures a spell cast newly makes legal, replacing
//! the "build a hypothetical position and run a full `legal_moves` rescan" approach
//! that dominated search time (36-41us per leaf node).
//!
//! See docs/superpowers/specs/2026-09-01-spell-capture-delta-design.md.

use crate::attacks::attackers_to;
use crate::bitboard::Bitboard;
use crate::board::Board;
use crate::legal::{in_baseline, pin_ray, pins_of, PinMap, SpellCast};
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

/// The maximum unfrozen sliders one side can have. A legal position tops out at 15
/// (16 pieces, one of them the king); `slider_scan` records the true count and
/// declines above this, which only a hand-built test board can reach.
const MAX_SLIDERS: usize = 16;

/// Our unfrozen sliders and the squares each attacks under the node's *current*
/// field. `jump_captures` diffs its post-jump attack set against these, and none of
/// it depends on which square is jumped -- so it is walked once per node rather than
/// once per jump target.
struct SliderScan {
    count: usize,
    items: [(Square, PieceKind, Bitboard); MAX_SLIDERS],
    /// The union of every recorded slider's attack set. A jump can only ever extend
    /// a ray *past* the jumped square, so a square outside this union is one no ray
    /// of ours reaches at all -- and `jump_captures` can answer "nothing gained" for
    /// it without walking pins. Empty when the scan overflowed.
    reach: Bitboard,
}

impl SliderScan {
    const EMPTY: SliderScan = SliderScan {
        count: 0,
        items: [(Square(0), PieceKind::Pawn, Bitboard::EMPTY); MAX_SLIDERS],
        reach: Bitboard::EMPTY,
    };

    fn recorded(&self) -> Option<&[(Square, PieceKind, Bitboard)]> {
        (self.count <= MAX_SLIDERS).then(|| &self.items[..self.count])
    }
}

fn slider_scan(board: &Board, us: Color, frozen: Bitboard, slider_occ: Bitboard) -> SliderScan {
    let bb = our_sliders(board, us, frozen);
    let mut scan = SliderScan {
        count: bb.count() as usize,
        items: [(Square(0), PieceKind::Pawn, Bitboard::EMPTY); MAX_SLIDERS],
        reach: Bitboard::EMPTY,
    };
    if scan.count <= MAX_SLIDERS {
        for (slot, p) in scan.items.iter_mut().zip(bb.iter()) {
            let kind = board.get(p).expect("slider bitboard square is occupied").kind;
            *slot = (p, kind, slider_attacks(kind, p, slider_occ));
            scan.reach = scan.reach.union(slot.2);
        }
    }
    scan
}

/// Squares whose jump could add a new attacker on our king, precomputed once
/// per node (see docs/superpowers/specs/2026-09-02-jump-exposure-precompute-
/// and-fuzz-harness-design.md). Jump only grants slider transparency, so the
/// only way it can add a checker is: `blocker` is the first REAL (non-
/// transparent) piece on one of the king's 8 rook/bishop lines, and beyond it
/// -- skipping over any square that already has a live jump field of its own,
/// since those don't block either -- sits at least one matching, unfrozen
/// enemy slider. `revealed` is every such square found along that line
/// (usually zero or one; more than one only when multiple pre-existing jump
/// fields are stacked on the same line). At most 8 entries: a square lies on
/// at most one of the king's 8 lines.
struct JumpExposure {
    mask: Bitboard,
    count: usize,
    pairs: [(Square, Bitboard); 8],
}

impl JumpExposure {
    const EMPTY: JumpExposure =
        JumpExposure { mask: Bitboard::EMPTY, count: 0, pairs: [(Square(0), Bitboard::EMPTY); 8] };

    /// The attacker(s) a jump on `s` would reveal (empty if none).
    fn revealed_by(&self, s: Square) -> Bitboard {
        self.pairs[..self.count]
            .iter()
            .find(|&&(blocker, _)| blocker == s)
            .map(|&(_, r)| r)
            .unwrap_or(Bitboard::EMPTY)
    }
}

fn jump_exposure_scan(
    board: &Board,
    king_sq: Square,
    enemy: Color,
    frozen: Bitboard,
    slider_occ: Bitboard,
    jump: Bitboard,
) -> JumpExposure {
    let real_occ = board.occupancy();
    let mut exp = JumpExposure::EMPTY;
    for &dir in crate::rays::ROOK_DIRS.iter().chain(crate::rays::BISHOP_DIRS.iter()) {
        let Some(blocker) =
            crate::rays::ray_attacks(king_sq, slider_occ, dir).intersect(slider_occ).iter().next()
        else {
            continue;
        };
        let is_rook_dir = crate::rays::ROOK_DIRS.contains(&dir);
        let mut revealed = Bitboard::EMPTY;
        let mut from = blocker;
        loop {
            let Some(next) =
                crate::rays::ray_attacks(from, real_occ, dir).intersect(real_occ).iter().next()
            else {
                break;
            };
            let piece = board.get(next).expect("real_occ square must be occupied");
            let kind_matches = if is_rook_dir {
                matches!(piece.kind, PieceKind::Rook | PieceKind::Queen)
            } else {
                matches!(piece.kind, PieceKind::Bishop | PieceKind::Queen)
            };
            if piece.color == enemy && !frozen.contains(next) && kind_matches {
                revealed = revealed.with(next);
            }
            if !jump.contains(next) {
                break;
            }
            from = next;
        }
        if !revealed.is_empty() {
            exp.mask = exp.mask.with(blocker);
            exp.pairs[exp.count] = (blocker, revealed);
            exp.count += 1;
        }
    }
    exp
}

/// Everything the delta needs that depends on the position but not on the cast.
/// `generate_quiescence_from` builds one per node and hands it to every cast it
/// tries. Before this existed each cast rebuilt all of it: a node with 30 freeze
/// targets recomputed `checkers` and a 520-byte `PinMap` thirty times over, and one
/// with 30 jump targets re-walked every one of our sliders' rays thirty times.
///
/// `generate_quiescence_from` also reads its own `frozen`/`jump`/`slider_occ`/
/// `checkers`/`pins` off this struct, so each value has exactly one computation site
/// and the delta cannot silently disagree with the `*_may_change_this_ply` guards
/// that decide whether to call it in the first place.
pub(crate) struct NodeContext {
    pub(crate) us: Color,
    pub(crate) enemy: Color,
    /// `None` only on a kingless board, which every path below declines on.
    pub(crate) king_sq: Option<Square>,
    pub(crate) frozen: Bitboard,
    pub(crate) jump: Bitboard,
    /// Occupancy with live jump squares removed, so slider rays run through them.
    pub(crate) slider_occ: Bitboard,
    pub(crate) enemy_bb: Bitboard,
    /// Attackers of our king under the current field; empty when we have no king.
    /// This is `freeze_captures`' `checkers_before`.
    pub(crate) checkers: Bitboard,
    /// Pins on our king under the current field -- `freeze_captures`' `pins_before`.
    pub(crate) pins: PinMap,
    sliders: SliderScan,
    jump_exposure: JumpExposure,
}

impl NodeContext {
    pub(crate) fn new(pos: &Position) -> NodeContext {
        let us = pos.side_to_move;
        let enemy = us.opposite();
        let frozen = crate::spells::frozen_bb(pos);
        let jump = crate::spells::jump_bb(pos);
        let slider_occ = pos.board.occupancy().minus(jump);
        let king_sq = pos.board.king_square(us);
        let (checkers, pins) = match king_sq {
            Some(k) => (
                attackers_to(pos, k, enemy, frozen, slider_occ),
                pins_of(pos, k, us, frozen, jump),
            ),
            None => (
                Bitboard::EMPTY,
                PinMap { pinned: Bitboard::EMPTY, rays: [Bitboard::EMPTY; 64] },
            ),
        };
        // Both `sliders` and `jump_exposure` are read only by `jump_captures`
        // (see its `ctx.sliders`/`ctx.jump_exposure` uses below), which every
        // caller already gates on this same `castable()` check before calling
        // `captures_enabled_by_in` with a jump cast -- so computing either when
        // jump is exhausted or on cooldown is pure waste.
        let jump_castable = pos.spells(us).jump.castable();
        let jump_exposure = match king_sq {
            Some(k) if jump_castable => jump_exposure_scan(&pos.board, k, enemy, frozen, slider_occ, jump),
            _ => JumpExposure::EMPTY,
        };
        let sliders = if jump_castable {
            slider_scan(&pos.board, us, frozen, slider_occ)
        } else {
            SliderScan::EMPTY
        };
        NodeContext {
            us,
            enemy,
            king_sq,
            frozen,
            jump,
            slider_occ,
            enemy_bb: pos.board.color_bb(enemy),
            checkers,
            pins,
            sliders,
            jump_exposure,
        }
    }
}

/// Captures that `cast` newly makes legal for `pos.side_to_move`, excluding
/// anything already legal in `baseline`. Appends to `out`.
///
/// Returning `NeedsRescan` is always safe: correctness never depends on this
/// function being exhaustive, only speed does.
///
/// Precondition for a `SpellKind::Jump` cast: `cast.square` must be one `pos
/// .side_to_move` could actually cast jump on right now, i.e. `pos.spells
/// (pos.side_to_move).jump.castable()` -- `NodeContext::new` skips the jump-only
/// precompute otherwise (jump is exhausted or on cooldown), and every existing
/// caller already satisfies this because it sources `cast.square` from
/// `spells::jump_targets`/`relevant_jump_targets`, both of which return empty
/// when the spell isn't castable.
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
    captures_enabled_by_in(&NodeContext::new(pos), pos, cast, baseline, out)
}

/// `captures_enabled_by` for a caller that is trying several casts against the same
/// position and has already built the `NodeContext`. `ctx` MUST be
/// `NodeContext::new(pos)` for this `pos`; passing a stale one is a correctness bug
/// the type system does not catch.
pub(crate) fn captures_enabled_by_in(
    ctx: &NodeContext,
    pos: &Position,
    cast: SpellCast,
    baseline: &[PieceMove],
    out: &mut Vec<PieceMove>,
) -> Delta {
    let start_len = out.len();
    let delta = match cast.kind {
        SpellKind::Jump => jump_captures(ctx, pos, cast.square, baseline, out),
        SpellKind::Freeze => freeze_captures(ctx, pos, cast.square, baseline, out),
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
    ctx: &NodeContext,
    pos: &Position,
    s: Square,
    baseline: &[PieceMove],
    out: &mut Vec<PieceMove>,
) -> Delta {
    let Some(king_sq) = ctx.king_sq else {
        return Delta::NeedsRescan;
    };
    let Some(sliders) = ctx.sliders.recorded() else {
        return Delta::NeedsRescan;
    };

    let jump_after = ctx.jump.with(s);
    // Equal to `occupancy().minus(jump_after)`: `ctx.slider_occ` already has every
    // previously live jump square subtracted, so `s` is the only one left to remove.
    let slider_occ_after = ctx.slider_occ.without(s);

    // Jump is symmetric: it can open an enemy slider onto our own king.
    // `ctx.jump_exposure` was precomputed once for the whole node (see
    // `jump_exposure_scan`) instead of walking `attackers_to` fresh for every
    // target: checkers_after is provably `ctx.checkers` unchanged unless `s`
    // is one of the (at most 8) squares that precompute recorded, and
    // `revealed_by` returns an empty Bitboard (a no-op union) when it isn't.
    let checkers_after = ctx.checkers.union(ctx.jump_exposure.revealed_by(s));
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

    // No ray of ours reaches `s`, so no ray of ours grows when `s` turns transparent
    // and `gained` is empty for every slider below -- the loop would emit nothing and
    // take none of its exits. Returning here instead of falling through skips the
    // `pins_of` walk, which is the expensive part of this function and is read only
    // inside that loop. This has to come *after* the double-check decline above:
    // that one is about a capture of the ENEMY king becoming legal, which has nothing
    // to do with where our own sliders point.
    if !ctx.sliders.reach.contains(s) {
        return Delta::Complete;
    }

    let single_checker = checkers_after.iter().next();
    let pins_after = pins_of(pos, king_sq, ctx.us, ctx.frozen, jump_after);

    for &(p, kind, before) in sliders {
        // A jump only ever extends a ray *past* `s`. A slider that did not already
        // reach `s` stops at an earlier blocker either way, so its attack set is
        // unchanged and `gained` would come out empty -- and every `NeedsRescan`
        // exit in this loop sits inside the `gained` walk, so skipping is exactly
        // equivalent to running the body, not an approximation of it.
        if !before.contains(s) {
            continue;
        }
        let gained =
            slider_attacks(kind, p, slider_occ_after).minus(before).intersect(ctx.enemy_bb);
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
            // Provably dead, so it is a debug-only check rather than a linear scan
            // on the hot path: `t` came out of `gained`, i.e. `p` did *not* attack
            // `t` before the jump, so no baseline move can run from `p` to `t`.
            // (`freeze_captures`' `in_baseline` calls below are live by contrast --
            // freeze leaves reachability alone, so its candidates were already
            // pseudo-legal and may well have been legal too.)
            debug_assert!(
                !in_baseline(baseline, mv),
                "jump delta re-emitted baseline move {mv:?}: `gained` was not disjoint from baseline",
            );
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
        // The next two arms are unreachable from the one caller today (the
        // released-pin loop in `freeze_captures`): a king is never its own pin
        // blocker, and a released pawn makes that loop decline before it gets here.
        // Kept rather than `unreachable!()` so a future caller passing an arbitrary
        // square gets an answer instead of a panic -- but note the pawn arm is only
        // *pseudo*-correct: it yields the capture squares and silently drops the four
        // promotion variants, which a new caller would have to expand itself.
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
/// That list is the spec. The `const _` assertion just below pins `legal_moves`'
/// filter count so that adding or removing a filter there breaks this build -- a
/// tripwire, not a proof, since it depends on whoever edits `legal_moves` updating
/// the count the comment there tells them to update.
const _: () = assert!(
    crate::legal::LEGAL_MOVE_FILTERS == 6,
    "legal_moves' filter list changed -- revisit freeze_captures' four-mechanism enumeration",
);

fn freeze_captures(
    ctx: &NodeContext,
    pos: &Position,
    s: Square,
    baseline: &[PieceMove],
    out: &mut Vec<PieceMove>,
) -> Delta {
    let Some(king_sq) = ctx.king_sq else {
        return Delta::NeedsRescan;
    };
    // See the sort just before the final `Delta::Complete`: this call's own slice of
    // `out` has to come out in `pseudo_legal_moves` order, and it is built in two
    // passes that do not interleave.
    let out_start = out.len();

    let frozen_before = ctx.frozen;
    let zone = crate::spells::FREEZE_ZONE[s.0 as usize];
    let frozen_after = frozen_before.union(zone);
    let enemy_bb = ctx.enemy_bb;
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

    let jump = ctx.jump;
    let slider_occ = ctx.slider_occ;

    if !ctx.checkers.is_empty() {
        // Check dispelled (or still live): the newly legal set is essentially the
        // whole position, which is not a cheap delta. This is what the escape
        // hatch exists for.
        //
        // Testing `ctx.checkers` (the pre-cast checkers) alone is exhaustive; there is no
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
    if let Some(their_king) = pos.board.king_square(ctx.enemy) {
        if !crate::attacks::attackers_to(pos, their_king, ctx.us, frozen_after, slider_occ).is_empty() {
            return Delta::NeedsRescan;
        }
    }

    let pins_before = &ctx.pins;
    let pins_after = pins_of(pos, king_sq, ctx.us, frozen_after, jump);

    // A piece that stays pinned, with the very same ray, can still gain a capture --
    // so neither `released` nor a ray comparison is enough on its own.
    let jump_enemy = jump.intersect(enemy_bb);
    for p in pins_after.pinned.iter() {
        // A pin ray can *grow* rather than vanish: a frozen pinner sitting on a live
        // jump square stops pinning but stays transparent, so `pins_of` walks on to a
        // further slider. The blocker is still pinned -- so it never reaches
        // `released` -- yet its ray, and with it its legal captures, just got longer.
        // (Ray *growth* is the only new-pin-ish case there is: a freeze can never
        // create a pin. `is_pinner` requires `!frozen` (legal.rs) and the blocker walk
        // ignores `frozen` entirely, so growing `frozen` only ever removes pinners --
        // `pins_after.pinned` is a subset of `pins_before.pinned`, and a square pinned
        // only *after* the cast does not exist.)
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
            if crate::legal::king_dest_safe(&hypo, king_sq, t, ctx.enemy) {
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

    /// The `out.truncate(start_len)` discipline only matters on a decline that happens
    /// *after* candidates were already pushed, and only one path reaches that: the
    /// released-pin loop walks `released` in ascending square order, so a non-pawn
    /// released below a released pawn emits its captures before the pawn forces the
    /// decline. This fixture builds exactly that.
    #[test]
    fn declining_after_pushing_candidates_restores_the_output_buffer() {
        // White Re5 is pinned to Ke4 by Re7 up the e-file; White Pf5 is pinned by Bg6
        // up the diagonal. freeze@f7 covers e7 AND g6, so both pins die at once and
        // `released` is {e5, f5}. e5 < f5, so the rook pushes Re5xa5 as a candidate
        // and *then* the pawn -- whose captures carry promotion and en-passant
        // variants this path does not model -- forces `NeedsRescan`.
        let mut pos = empty_board();
        put(&mut pos, "e4", Color::White, PieceKind::King);
        put(&mut pos, "e5", Color::White, PieceKind::Rook);
        put(&mut pos, "f5", Color::White, PieceKind::Pawn);
        put(&mut pos, "e7", Color::Black, PieceKind::Rook);
        put(&mut pos, "g6", Color::Black, PieceKind::Bishop);
        put(&mut pos, "a5", Color::Black, PieceKind::Rook);
        put(&mut pos, "a8", Color::Black, PieceKind::King);

        let baseline = legal_moves(&pos);
        // Off the pin ray, so it is the freeze that legalises it -- Re5xe7 (the pinner
        // itself) was always legal and is filtered out by `in_baseline`.
        let rxa5 = PieceMove::quiet(sq("e5"), sq("a5"));
        assert!(!baseline.contains(&rxa5), "fixture is wrong: Re5xa5 must start pinned-illegal");
        let cast = SpellCast { kind: SpellKind::Freeze, square: sq("f7") };
        assert!(
            rescan_oracle(&pos, cast, &baseline).contains(&rxa5),
            "fixture is wrong: freeze@f7 must enable Re5xa5",
        );

        // White box, so this test cannot silently go vacuous again: the inner
        // mechanism really does leave a candidate behind when it declines.
        let mut raw = Vec::new();
        let ctx = NodeContext::new(&pos);
        assert_eq!(freeze_captures(&ctx, &pos, sq("f7"), &baseline, &mut raw), Delta::NeedsRescan);
        assert_eq!(raw, vec![rxa5], "fixture is wrong: the decline must follow a push");

        // ...and the public entry point hands the caller its buffer back untouched.
        let sentinel: Vec<PieceMove> = baseline.iter().take(2).copied().collect();
        assert!(!sentinel.is_empty(), "fixture is wrong: need a non-empty prior buffer");
        let mut out = sentinel.clone();
        assert_eq!(captures_enabled_by(&pos, cast, &baseline, &mut out), Delta::NeedsRescan);
        assert_eq!(out, sentinel, "a declining call must restore `out` to its prior contents");
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

    // -----------------------------------------------------------------------
    // Jump-exposure precompute characterization fixtures. These pin the rule
    // from the design doc *before* the precompute exists (Task 2 of the
    // implementation plan): every one of these must already pass against the
    // pre-precompute `jump_captures`, which computes the same answer via a
    // fresh `attackers_to` call per target. They exist to catch a regression
    // in the refactor, not to fix a bug.
    // -----------------------------------------------------------------------

    /// King d1's file ray is blocked by an enemy knight on d4 (blocker color is
    /// irrelevant to jump transparency), with a rook behind it on d8 -- jumping
    /// d4 must reveal exactly that rook as a checker. White's bishop on a1
    /// independently gains Nf6 through the same jump (its own a1-h8 diagonal
    /// also passes through d4), which is unrelated to the check and must be
    /// filtered: only a capture of the checker (d8) is a legal response to
    /// being in check.
    #[test]
    fn jump_exposure_on_a_rook_line_filters_captures_to_the_new_checker() {
        let mut pos = empty_board();
        put(&mut pos, "d1", Color::White, PieceKind::King);
        put(&mut pos, "d4", Color::Black, PieceKind::Knight);
        put(&mut pos, "d8", Color::Black, PieceKind::Rook);
        put(&mut pos, "a1", Color::White, PieceKind::Bishop);
        put(&mut pos, "f6", Color::Black, PieceKind::Knight);
        put(&mut pos, "h8", Color::Black, PieceKind::King);

        let baseline = legal_moves(&pos);
        let cast = SpellCast { kind: SpellKind::Jump, square: sq("d4") };
        let bxf6 = PieceMove::quiet(sq("a1"), sq("f6"));
        assert!(
            !rescan_oracle(&pos, cast, &baseline).contains(&bxf6),
            "fixture is wrong: rescan must filter Bxf6 once jump@d4 lets Rd8 check Kd1",
        );
        assert_delta_sound(&pos, cast);
    }

    /// Same mechanism, diagonal ray: king e1's (-1,+1) diagonal is blocked by a
    /// knight on c3 with a bishop behind it on a5. White's queen on e5
    /// independently gains Qxa1 through the same jump (its own diagonal through
    /// c3 in the *other* direction), which must be filtered the same way.
    #[test]
    fn jump_exposure_on_a_bishop_line_filters_captures_to_the_new_checker() {
        let mut pos = empty_board();
        put(&mut pos, "e1", Color::White, PieceKind::King);
        put(&mut pos, "c3", Color::Black, PieceKind::Knight);
        put(&mut pos, "a5", Color::Black, PieceKind::Bishop);
        put(&mut pos, "e5", Color::White, PieceKind::Queen);
        put(&mut pos, "a1", Color::Black, PieceKind::Rook);
        put(&mut pos, "h8", Color::Black, PieceKind::King);

        let baseline = legal_moves(&pos);
        let cast = SpellCast { kind: SpellKind::Jump, square: sq("c3") };
        let qxa1 = PieceMove::quiet(sq("e5"), sq("a1"));
        assert!(
            !rescan_oracle(&pos, cast, &baseline).contains(&qxa1),
            "fixture is wrong: rescan must filter Qxa1 once jump@c3 lets Ba5 check Ke1",
        );
        assert_delta_sound(&pos, cast);
    }

    /// King d1 is already in check from a knight on b2 (unrelated to any spell).
    /// jump@d4 additionally exposes Rd8 down the file -- turning a single check
    /// into a double check, where only king moves are legal. `jump_captures`
    /// must decline rather than reason about double check itself.
    #[test]
    fn jump_exposure_combined_with_a_pre_existing_checker_declines() {
        let mut pos = empty_board();
        put(&mut pos, "d1", Color::White, PieceKind::King);
        put(&mut pos, "b2", Color::Black, PieceKind::Knight);
        put(&mut pos, "d4", Color::Black, PieceKind::Knight);
        put(&mut pos, "d8", Color::Black, PieceKind::Rook);
        put(&mut pos, "h8", Color::Black, PieceKind::King);

        let cast = SpellCast { kind: SpellKind::Jump, square: sq("d4") };
        let hypothetical = crate::legal::position_with_field(&pos, cast);
        let hyp_moves = legal_moves(&hypothetical);
        assert!(
            !hyp_moves.is_empty() && hyp_moves.iter().all(|mv| mv.from == sq("d1")),
            "fixture is wrong: jump@d4 must leave only king moves (double check), got {hyp_moves:?}",
        );

        let baseline = legal_moves(&pos);
        let mut out = Vec::new();
        assert_eq!(
            captures_enabled_by(&pos, cast, &baseline, &mut out),
            Delta::NeedsRescan,
            "a jump that creates a double check must decline, not guess",
        );
        assert!(out.is_empty(), "a declining call must not touch `out`");
    }

    /// Same geometry as the rook-line fixture, but the piece behind the
    /// blocker is a Bishop -- the wrong kind for a straight-line ray. It must
    /// not be treated as a newly-revealed checker, so the unrelated Bxf6
    /// capture stays available.
    #[test]
    fn jump_exposure_requires_the_revealed_piece_kind_to_match_the_ray() {
        let mut pos = empty_board();
        put(&mut pos, "d1", Color::White, PieceKind::King);
        put(&mut pos, "d4", Color::Black, PieceKind::Knight);
        put(&mut pos, "d8", Color::Black, PieceKind::Bishop);
        put(&mut pos, "a1", Color::White, PieceKind::Bishop);
        put(&mut pos, "f6", Color::Black, PieceKind::Knight);
        put(&mut pos, "h8", Color::Black, PieceKind::King);

        let baseline = legal_moves(&pos);
        let cast = SpellCast { kind: SpellKind::Jump, square: sq("d4") };
        let bxf6 = PieceMove::quiet(sq("a1"), sq("f6"));
        assert!(
            rescan_oracle(&pos, cast, &baseline).contains(&bxf6),
            "fixture is wrong: a bishop on d8 cannot check Kd1 down the file, Bxf6 must stay legal",
        );
        assert_delta_sound(&pos, cast);
    }

    /// Same geometry as the rook-line fixture, but Rd8 is frozen by a
    /// pre-existing field. A frozen piece "exerts no control at all"
    /// (rules/30-freeze.md), so it must not be treated as a newly-revealed
    /// checker even though it is the right kind and color.
    #[test]
    fn a_frozen_revealed_piece_does_not_count_as_exposure() {
        let mut pos = empty_board();
        put(&mut pos, "d1", Color::White, PieceKind::King);
        put(&mut pos, "d4", Color::Black, PieceKind::Knight);
        put(&mut pos, "d8", Color::Black, PieceKind::Rook);
        put(&mut pos, "a1", Color::White, PieceKind::Bishop);
        put(&mut pos, "f6", Color::Black, PieceKind::Knight);
        put(&mut pos, "h8", Color::Black, PieceKind::King);
        add_field(&mut pos, "d8", Color::White, SpellKind::Freeze);

        let baseline = legal_moves(&pos);
        let cast = SpellCast { kind: SpellKind::Jump, square: sq("d4") };
        let bxf6 = PieceMove::quiet(sq("a1"), sq("f6"));
        assert!(
            rescan_oracle(&pos, cast, &baseline).contains(&bxf6),
            "fixture is wrong: a frozen Rd8 cannot check Kd1, Bxf6 must stay legal",
        );
        assert_delta_sound(&pos, cast);
    }

    /// Same geometry again, but Rd8 is White's own piece, not an enemy's. Only
    /// an enemy piece can check our king.
    #[test]
    fn an_own_colored_revealed_piece_does_not_count_as_exposure() {
        let mut pos = empty_board();
        put(&mut pos, "d1", Color::White, PieceKind::King);
        put(&mut pos, "d4", Color::Black, PieceKind::Knight);
        put(&mut pos, "d8", Color::White, PieceKind::Rook);
        put(&mut pos, "a1", Color::White, PieceKind::Bishop);
        put(&mut pos, "f6", Color::Black, PieceKind::Knight);
        put(&mut pos, "h8", Color::Black, PieceKind::King);

        let baseline = legal_moves(&pos);
        let cast = SpellCast { kind: SpellKind::Jump, square: sq("d4") };
        let bxf6 = PieceMove::quiet(sq("a1"), sq("f6"));
        assert!(
            rescan_oracle(&pos, cast, &baseline).contains(&bxf6),
            "fixture is wrong: White's own Rd8 cannot check Kd1, Bxf6 must stay legal",
        );
        assert_delta_sound(&pos, cast);
    }

    /// Same geometry with d8 empty entirely -- the king's ray runs off the
    /// board with only one blocker (d4) and nothing behind it. Must not panic
    /// and must not treat d4 as exposed.
    #[test]
    fn a_ray_with_no_second_blocker_creates_no_exposure() {
        let mut pos = empty_board();
        put(&mut pos, "d1", Color::White, PieceKind::King);
        put(&mut pos, "d4", Color::Black, PieceKind::Knight);
        put(&mut pos, "a1", Color::White, PieceKind::Bishop);
        put(&mut pos, "f6", Color::Black, PieceKind::Knight);
        put(&mut pos, "h8", Color::Black, PieceKind::King);

        let baseline = legal_moves(&pos);
        let cast = SpellCast { kind: SpellKind::Jump, square: sq("d4") };
        let bxf6 = PieceMove::quiet(sq("a1"), sq("f6"));
        assert!(
            rescan_oracle(&pos, cast, &baseline).contains(&bxf6),
            "fixture is wrong: with nothing behind d4, Bxf6 must stay legal",
        );
        assert_delta_sound(&pos, cast);
    }

    /// Direct check of the precompute itself, reusing the rook-line fixture's
    /// geometry: d4 must be recorded as exposing d8, and no other square on
    /// the board should be recorded as exposing anything.
    #[test]
    fn jump_exposure_scan_finds_exactly_the_blocker_and_its_revealed_attacker() {
        let mut pos = empty_board();
        put(&mut pos, "d1", Color::White, PieceKind::King);
        put(&mut pos, "d4", Color::Black, PieceKind::Knight);
        put(&mut pos, "d8", Color::Black, PieceKind::Rook);
        put(&mut pos, "h8", Color::Black, PieceKind::King);

        let ctx = NodeContext::new(&pos);
        assert_eq!(ctx.jump_exposure.revealed_by(sq("d4")), Bitboard::from_square(sq("d8")));
        assert_eq!(ctx.jump_exposure.mask.count(), 1, "only d4 should be recorded as exposed");
    }

    /// A pre-existing jump field on the REVEALED square itself: jumping g3
    /// (Black's own queen) opens the g-file toward White's rook on g5 -- but
    /// g5 already has its own live jump field, so it does not block anything
    /// and must still count as an attacker once revealed. Combined with a
    /// pre-existing checker (White's knight on h4, unrelated to any spell),
    /// jump@g3 must decline as a double check. This reproduces a real bug an
    /// earlier version of `jump_exposure_scan` had: it used `slider_occ`
    /// (which excludes g5, since g5 is already transparent) both to walk the
    /// ray *and* to test "is a piece here," so it was blind to any piece
    /// standing on an already-transparent square -- see the ruling in this
    /// plan's Task 2 for the fix.
    #[test]
    fn jump_exposure_sees_past_an_already_transparent_revealed_piece() {
        let mut pos = empty_board();
        put(&mut pos, "g2", Color::Black, PieceKind::King);
        put(&mut pos, "g3", Color::Black, PieceKind::Queen);
        put(&mut pos, "h4", Color::White, PieceKind::Knight);
        put(&mut pos, "g5", Color::White, PieceKind::Rook);
        put(&mut pos, "e1", Color::White, PieceKind::King);
        pos.side_to_move = Color::Black;
        add_field(&mut pos, "g5", Color::White, SpellKind::Jump);

        let ctx = NodeContext::new(&pos);
        assert_eq!(
            ctx.jump_exposure.revealed_by(sq("g3")),
            Bitboard::from_square(sq("g5")),
            "fixture is wrong: g5's rook must be found even though its own square is transparent",
        );

        let cast = SpellCast { kind: SpellKind::Jump, square: sq("g3") };
        let hypothetical = crate::legal::position_with_field(&pos, cast);
        let hyp_moves = legal_moves(&hypothetical);
        // Double check (Nh4 + Rg5) restricts Black to king moves -- plus, in this
        // position specifically, Qxe1. King capture is legal exactly when
        // `after_count <= before_count.max(1)` (`legal.rs`'s king-capture check,
        // ~line 120; see `rules/50-interactions.md`'s "Win conditions" section for
        // the `[VERIFIED]` rule this implements) -- it is NOT true in general that
        // double check permits capturing the enemy king. It holds here because, in
        // the hypothetical (jump@g3 cast, before the piece move), g5 -- the rook's
        // square, which already carries the pre-existing live jump field set up by
        // `add_field` above -- is transparent independently of g3. So Rg5 already
        // checks through both transparent squares (g3 from the cast under test, g5
        // from the pre-existing field) before the queen ever moves. `Qg3xe1` then
        // vacates g3, but g3's transparency had already made it non-blocking, so
        // physically emptying it changes nothing: attackers_after == attackers_before
        // == 2 <= max(2, 1). This fixture's point is that nothing *else* (no
        // interposition, no other capture) is legal, which is what actually
        // distinguishes single from double check.
        assert!(
            !hyp_moves.is_empty()
                && hyp_moves.iter().all(|mv| {
                    mv.from == sq("g2")
                        || pos.board.get(mv.to).is_some_and(|p| p.kind == PieceKind::King)
                }),
            "fixture is wrong: jump@g3 must leave only king moves and/or capturing the enemy king \
             (double check from Nh4 and Rg5), got {hyp_moves:?}",
        );

        let baseline = legal_moves(&pos);
        let mut out = Vec::new();
        assert_eq!(
            captures_enabled_by(&pos, cast, &baseline, &mut out),
            Delta::NeedsRescan,
            "a jump that reveals a checker sitting on an already-transparent square must still decline",
        );
        assert!(out.is_empty(), "a declining call must not touch `out`");
    }

    /// The walk-through-transparency guard itself, isolated: a WRONG-KIND
    /// piece (a knight, which can never check via a straight-line ray) sits
    /// on its own transparent square between the blocker and the real
    /// attacker. The scan must not stop at the knight -- it doesn't match,
    /// but it also doesn't block (its own square is jump-transparent), so
    /// the walk must continue past it to find the rook. This is the fixture
    /// `jump_exposure_sees_past_an_already_transparent_revealed_piece` above
    /// cannot cover: that one only ever needs a single hop (g4 is empty
    /// there), so it stays green even if the "keep walking" step is deleted
    /// entirely. This one needs two hops and dies if it is (found by task
    /// review during Task 2, 2026-09-02 -- see the ruling above this test).
    #[test]
    fn jump_exposure_walks_past_a_non_matching_piece_on_a_transparent_square() {
        let mut pos = empty_board();
        put(&mut pos, "g2", Color::Black, PieceKind::King);
        put(&mut pos, "g3", Color::Black, PieceKind::Queen);
        put(&mut pos, "g4", Color::White, PieceKind::Knight);
        put(&mut pos, "g5", Color::White, PieceKind::Rook);
        put(&mut pos, "e1", Color::White, PieceKind::King);
        pos.side_to_move = Color::Black;
        add_field(&mut pos, "g4", Color::White, SpellKind::Jump);

        let ctx = NodeContext::new(&pos);
        assert_eq!(
            ctx.jump_exposure.revealed_by(sq("g3")),
            Bitboard::from_square(sq("g5")),
            "fixture is wrong: the walk must skip the non-matching knight on g4 (transparent) and find the rook on g5",
        );
    }
}

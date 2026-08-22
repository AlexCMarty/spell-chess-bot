use crate::bitboard::Bitboard;
use crate::movegen::PieceMove;
use crate::position::{Position, SpellField, SpellKind};
use crate::types::{Color, PieceKind, Square};

pub fn freeze_zone(target: Square) -> Vec<Square> {
    let mut out = Vec::new();
    let (tf, tr) = (target.file() as i8, target.rank() as i8);
    for df in -1..=1 {
        for dr in -1..=1 {
            let (f, r) = (tf + df, tr + dr);
            if (0..8).contains(&f) && (0..8).contains(&r) {
                out.push(Square::new(f as u8, r as u8));
            }
        }
    }
    out
}

const fn freeze_zone_bits(idx: u8) -> u64 {
    let tf = (idx % 8) as i8;
    let tr = (idx / 8) as i8;
    let mut bits = 0u64;
    let mut df = -1i8;
    while df <= 1 {
        let mut dr = -1i8;
        while dr <= 1 {
            let f = tf + df;
            let r = tr + dr;
            if f >= 0 && f < 8 && r >= 0 && r < 8 {
                bits |= 1u64 << (r * 8 + f) as u32;
            }
            dr += 1;
        }
        df += 1;
    }
    bits
}

pub const FREEZE_ZONE: [Bitboard; 64] = {
    let mut table = [Bitboard(0); 64];
    let mut i = 0;
    while i < 64 {
        table[i] = Bitboard(freeze_zone_bits(i as u8));
        i += 1;
    }
    table
};

fn field_active(pos: &Position, field: &SpellField) -> bool {
    pos.ply <= field.expires_after_ply
}

pub fn frozen_bb(pos: &Position) -> Bitboard {
    let mut acc = Bitboard::EMPTY;
    for f in pos.fields.iter() {
        if f.kind == SpellKind::Freeze && field_active(pos, f) {
            acc = acc.union(FREEZE_ZONE[f.square.0 as usize]);
        }
    }
    acc
}

pub fn jump_bb(pos: &Position) -> Bitboard {
    let mut acc = Bitboard::EMPTY;
    for f in pos.fields.iter() {
        if f.kind == SpellKind::Jump && field_active(pos, f) {
            acc = acc.with(f.square);
        }
    }
    acc
}

pub fn is_square_frozen(pos: &Position, square: Square) -> bool {
    frozen_bb(pos).contains(square)
}

// A square already carrying a live field of a given kind is not a legal target for that
// same kind again -- confirmed against the real engine (neither owner-specific nor
// affected by the other spell type), see rules/30-freeze.md and rules/40-jump.md.
fn is_field_anchor(pos: &Position, square: Square, kind: SpellKind) -> bool {
    pos.fields.iter().any(|f| f.kind == kind && field_active(pos, f) && f.square == square)
}

pub fn freeze_targets(pos: &Position, color: Color) -> Vec<Square> {
    if !pos.spells(color).freeze.castable() {
        return Vec::new();
    }
    (0..64).map(Square).filter(|&sq| !is_field_anchor(pos, sq, SpellKind::Freeze)).collect()
}

pub fn is_square_jump_active(pos: &Position, square: Square) -> bool {
    jump_bb(pos).contains(square)
}

pub fn jump_targets(pos: &Position, color: Color) -> Vec<Square> {
    if !pos.spells(color).jump.castable() {
        return Vec::new();
    }
    (0..64).map(Square).filter(|&sq| pos.board.get(sq).is_some() && !is_square_jump_active(pos, sq)).collect()
}

// `scan_pos` supplies piece positions and ray-walking occupancy (it may be a
// hypothetical post-move board); `castable_at` gates which blocker squares are
// actually valid jump-cast targets, which is always the *original* pre-move board --
// jump_targets is only ever defined over squares occupied before the mover's move,
// so a square that's only occupied in a hypothetical (e.g. a move's destination) is
// never a real candidate even if it would be a first-blocker after that move.
fn first_occupied_on_ray(pos: &Position, from: Square, dir: (i8, i8)) -> Option<Square> {
    let occ = pos.board.occupancy().minus(jump_bb(pos));
    let mut f = from.file() as i8 + dir.0;
    let mut r = from.rank() as i8 + dir.1;
    while (0..8).contains(&f) && (0..8).contains(&r) {
        let sq = Square::new(f as u8, r as u8);
        if occ.contains(sq) {
            return Some(sq);
        }
        f += dir.0;
        r += dir.1;
    }
    None
}

fn mark_jump_blockers(scan_pos: &Position, castable_at: &Position, sq: Square, dirs: &[(i8, i8)], relevant: &mut Bitboard) {
    for &dir in dirs {
        // Mark the first blocker *and* the one behind it: the field survives
        // for the caster's own move plus the opponent's one reply, so at most
        // one more piece (the opponent's own moving piece) can vacate the
        // first blocker's square within the field's lifetime, exposing
        // whatever sits behind it on the same ray to a check through the
        // jump-transparent target.
        if let Some(first) = first_occupied_on_ray(scan_pos, sq, dir) {
            if castable_at.board.get(first).is_some() {
                *relevant = relevant.with(first);
            }
            if let Some(second) = first_occupied_on_ray(scan_pos, first, dir) {
                if castable_at.board.get(second).is_some() {
                    *relevant = relevant.with(second);
                }
            }
        }
    }
}

fn mark_jump_relevant_squares(scan_pos: &Position, castable_at: &Position, relevant: &mut Bitboard) {
    let sliders = scan_pos.board.kind_bb(PieceKind::Bishop)
        .union(scan_pos.board.kind_bb(PieceKind::Rook))
        .union(scan_pos.board.kind_bb(PieceKind::Queen));
    for sq in sliders.iter() {
        let piece = scan_pos.board.get(sq).unwrap();

        // is_square_attacked detects a slider attacker by walking a ray *from the
        // target square* and taking whatever it finds last -- exactly the same
        // walk_ray used for movement. If the slider itself sits on a jump-active
        // square, that walk sees straight through it and reports whatever (if
        // anything) is behind it instead, hiding the slider as an attacker. So any
        // square a slider occupies is always a relevant jump target in its own
        // right, independent of whether it blocks anyone else's ray.
        if castable_at.board.get(sq).is_some() {
            *relevant = relevant.with(sq);
        }

        if matches!(piece.kind, PieceKind::Rook | PieceKind::Queen) {
            mark_jump_blockers(scan_pos, castable_at, sq, &crate::rays::ROOK_DIRS, relevant);
        }
        if matches!(piece.kind, PieceKind::Bishop | PieceKind::Queen) {
            mark_jump_blockers(scan_pos, castable_at, sq, &crate::rays::BISHOP_DIRS, relevant);
        }
    }

    for sq in scan_pos.board.kind_bb(PieceKind::Pawn).iter() {
        let piece = scan_pos.board.get(sq).unwrap();
        let start_rank: u8 = if piece.color == Color::White { 1 } else { 6 };
        if sq.rank() != start_rank {
            continue;
        }
        let dir: i8 = if piece.color == Color::White { 1 } else { -1 };
        let mid = Square::new(sq.file(), (sq.rank() as i8 + dir) as u8);
        if castable_at.board.get(mid).is_some() {
            *relevant = relevant.with(mid);
        }
    }
}

/// A jump cast only changes anything if the target is the first blocker on some
/// slider's ray (either color), a slider's own square (see
/// `mark_jump_relevant_squares`'s self-hiding note), or a pawn's double-step mid
/// square (either color) -- see
/// docs/superpowers/specs/2026-08-12-search-branching-factor-design.md.
///
/// All three are checked on the current board *and* on the board as it would look
/// after each of the mover's own candidate moves this turn: a slider arriving at
/// (or vacating) a square can create a new first-blocker relationship that didn't
/// exist before the move, and the cast-then-move turn is evaluated as a whole.
///
/// A square that already carries a live jump field is never a legal target at all
/// (confirmed against the real engine, see rules/40-jump.md) -- not merely
/// irrelevant, illegal -- so it's filtered out at the end regardless of how it was
/// marked.
pub fn relevant_jump_targets(pos: &Position, color: Color, baseline: &[PieceMove]) -> Vec<Square> {
    if !pos.spells(color).jump.castable() {
        return Vec::new();
    }
    let mut relevant = Bitboard::EMPTY;

    mark_jump_relevant_squares(pos, pos, &mut relevant);
    for mv in baseline {
        let hypothetical = crate::legal::apply_move_only(pos, mv);
        mark_jump_relevant_squares(&hypothetical, pos, &mut relevant);
    }
    relevant.minus(jump_bb(pos)).iter().collect()
}

/// A freeze cast only changes anything if its 3x3 zone touches a square that's
/// either occupied now or reachable by one of the mover's own legal moves this
/// turn (including a castling move's rook-landing square) -- see
/// docs/superpowers/specs/2026-08-12-search-branching-factor-design.md.
pub fn relevant_freeze_targets(pos: &Position, color: Color, baseline: &[PieceMove]) -> Vec<Square> {
    if !pos.spells(color).freeze.castable() {
        return Vec::new();
    }
    let mut landing = pos.board.occupancy();
    for mv in baseline {
        landing = landing.with(mv.to);
        if mv.is_castle {
            let rank = mv.from.rank();
            let rook_to = if mv.to.file() == 6 { Square::new(5, rank) } else { Square::new(3, rank) };
            landing = landing.with(rook_to);
        }
    }

    let mut relevant = Bitboard::EMPTY;
    for sq in landing.iter() {
        relevant = relevant.union(FREEZE_ZONE[sq.0 as usize]);
    }
    // A square already anchoring a live freeze field is never a legal target (see
    // rules/30-freeze.md), regardless of zone geometry.
    relevant.iter().filter(|&sq| !is_field_anchor(pos, sq, SpellKind::Freeze)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Square;

    #[test]
    fn corner_freeze_clips_to_2x2() {
        let a1 = Square::from_str("a1").unwrap();
        let zone = freeze_zone(a1);
        assert!(zone.contains(&Square::from_str("a1").unwrap()));
        assert!(zone.contains(&Square::from_str("a2").unwrap()));
        assert!(zone.contains(&Square::from_str("b2").unwrap()));
        assert!(!zone.contains(&Square::from_str("c3").unwrap()));
        assert_eq!(zone.len(), 4);
    }

    #[test]
    fn center_freeze_covers_3x3() {
        assert_eq!(freeze_zone(Square::from_str("d5").unwrap()).len(), 9);
    }

    #[test]
    fn relevant_jump_targets_is_a_subset_of_jump_targets() {
        let pos = Position::starting();
        let baseline = crate::legal::legal_moves(&pos);
        let exhaustive = jump_targets(&pos, Color::White);
        let relevant = relevant_jump_targets(&pos, Color::White, &baseline);
        for sq in &relevant {
            assert!(exhaustive.contains(sq));
        }
        assert!(relevant.len() < exhaustive.len());
    }

    #[test]
    fn b1_knight_is_a_first_blocker_in_the_starting_position() {
        // a1's rook rank-ray immediately hits b1, on the board as it stands --
        // true regardless of what the mover does with their move this turn.
        let pos = Position::starting();
        let baseline = crate::legal::legal_moves(&pos);
        let relevant = relevant_jump_targets(&pos, Color::White, &baseline);
        assert!(relevant.contains(&Square::from_str("b1").unwrap()));
    }

    #[test]
    fn relevant_jump_targets_respects_castable_gate() {
        let mut pos = Position::starting();
        pos.white_spells.jump.count = 0;
        let baseline = crate::legal::legal_moves(&pos);
        assert!(relevant_jump_targets(&pos, Color::White, &baseline).is_empty());
    }

    fn sparse_endgame() -> Position {
        let mut pos = Position { board: crate::board::Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(crate::types::Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(crate::types::Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(crate::types::Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("h8").unwrap(), Some(crate::types::Piece { color: Color::Black, kind: PieceKind::Rook }));
        pos
    }

    #[test]
    fn relevant_freeze_targets_is_a_subset_of_freeze_targets() {
        let pos = sparse_endgame();
        let baseline = crate::legal::legal_moves(&pos);
        let exhaustive = freeze_targets(&pos, Color::White);
        let relevant = relevant_freeze_targets(&pos, Color::White, &baseline);
        for sq in &relevant {
            assert!(exhaustive.contains(sq));
        }
        assert!(relevant.len() < exhaustive.len());
    }

    #[test]
    fn f5_is_excluded_when_unreachable_in_a_sparse_endgame() {
        // White's only this-turn landing squares are the fully-open a-file (the
        // rook), b1/c1/d1 (the rook along rank 1, blocked by its own king), and
        // d1/d2/e2/f1/f2 (the king) -- plus the occupied squares e1/a1/e8/h8.
        // f5's 3x3 zone (e4-g6) touches none of that.
        let pos = sparse_endgame();
        let baseline = crate::legal::legal_moves(&pos);
        let relevant = relevant_freeze_targets(&pos, Color::White, &baseline);
        assert!(!relevant.contains(&Square::from_str("f5").unwrap()));
    }

    #[test]
    fn relevant_freeze_targets_respects_castable_gate() {
        let pos = sparse_endgame();
        let baseline = crate::legal::legal_moves(&pos);
        let mut gated = pos.clone();
        gated.white_spells.freeze.count = 0;
        assert!(relevant_freeze_targets(&gated, Color::White, &baseline).is_empty());
    }

    #[test]
    fn frozen_bb_covers_the_3x3_and_not_beyond() {
        let mut pos = Position { board: crate::board::Board::empty(), ..Position::starting() };
        pos.fields.push(SpellField {
            square: Square::from_str("d5").unwrap(), owner: Color::White,
            kind: SpellKind::Freeze, expires_after_ply: pos.ply + 1,
        });
        let frozen = frozen_bb(&pos);
        assert!(frozen.contains(Square::from_str("d5").unwrap()));
        assert!(frozen.contains(Square::from_str("e6").unwrap()));
        assert!(!frozen.contains(Square::from_str("f7").unwrap()));
        assert_eq!(frozen.count(), 9);
    }

    #[test]
    fn jump_bb_is_exactly_the_anchor_square() {
        let mut pos = Position { board: crate::board::Board::empty(), ..Position::starting() };
        pos.fields.push(SpellField {
            square: Square::from_str("d4").unwrap(), owner: Color::White,
            kind: SpellKind::Jump, expires_after_ply: pos.ply + 1,
        });
        let jump = jump_bb(&pos);
        assert_eq!(jump, Bitboard::from_square(Square::from_str("d4").unwrap()));
        assert!(is_square_jump_active(&pos, Square::from_str("d4").unwrap()));
        assert!(!is_square_jump_active(&pos, Square::from_str("d5").unwrap()));
    }

    #[test]
    fn freeze_zone_table_matches_freeze_zone_for_every_square() {
        for i in 0..64u8 {
            let sq = Square(i);
            let mut from_vec = crate::bitboard::Bitboard::EMPTY;
            for z in freeze_zone(sq) {
                from_vec = from_vec.with(z);
            }
            assert_eq!(FREEZE_ZONE[i as usize], from_vec, "mismatch at {sq}");
        }
    }
}

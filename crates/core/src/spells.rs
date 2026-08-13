use std::collections::BTreeSet;
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

fn field_active(pos: &Position, field: &SpellField) -> bool {
    pos.ply <= field.expires_after_ply
}

pub fn is_square_frozen(pos: &Position, square: Square) -> bool {
    pos.fields.iter().any(|f| f.kind == SpellKind::Freeze && field_active(pos, f) && freeze_zone(f.square).contains(&square))
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
    is_field_anchor(pos, square, SpellKind::Jump)
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
fn mark_jump_relevant_squares(scan_pos: &Position, castable_at: &Position, relevant: &mut BTreeSet<Square>) {
    for i in 0..64u8 {
        let sq = Square(i);
        let Some(piece) = scan_pos.board.get(sq) else { continue };

        // is_square_attacked detects a slider attacker by walking a ray *from the
        // target square* and taking whatever it finds last -- exactly the same
        // walk_ray used for movement. If the slider itself sits on a jump-active
        // square, that walk sees straight through it and reports whatever (if
        // anything) is behind it instead, hiding the slider as an attacker. So any
        // square a slider occupies is always a relevant jump target in its own
        // right, independent of whether it blocks anyone else's ray.
        if matches!(piece.kind, PieceKind::Rook | PieceKind::Bishop | PieceKind::Queen)
            && castable_at.board.get(sq).is_some()
        {
            relevant.insert(sq);
        }

        let mut dirs: Vec<(i8, i8)> = Vec::new();
        if matches!(piece.kind, PieceKind::Rook | PieceKind::Queen) {
            dirs.extend_from_slice(&crate::rays::ROOK_DIRS);
        }
        if matches!(piece.kind, PieceKind::Bishop | PieceKind::Queen) {
            dirs.extend_from_slice(&crate::rays::BISHOP_DIRS);
        }
        for dir in dirs {
            // Mark the first blocker *and* the one behind it: the field survives
            // for the caster's own move plus the opponent's one reply, so at most
            // one more piece (the opponent's own moving piece) can vacate the
            // first blocker's square within the field's lifetime, exposing
            // whatever sits behind it on the same ray to a check through the
            // jump-transparent target.
            if let Some(&first) = crate::rays::walk_ray(scan_pos, sq, dir).last() {
                if castable_at.board.get(first).is_some() {
                    relevant.insert(first);
                }
                if let Some(&second) = crate::rays::walk_ray(scan_pos, first, dir).last() {
                    if castable_at.board.get(second).is_some() {
                        relevant.insert(second);
                    }
                }
            }
        }
    }

    for i in 0..64u8 {
        let sq = Square(i);
        let Some(piece) = scan_pos.board.get(sq) else { continue };
        if piece.kind != PieceKind::Pawn {
            continue;
        }
        let start_rank: u8 = if piece.color == Color::White { 1 } else { 6 };
        if sq.rank() != start_rank {
            continue;
        }
        let dir: i8 = if piece.color == Color::White { 1 } else { -1 };
        let mid = Square::new(sq.file(), (sq.rank() as i8 + dir) as u8);
        if castable_at.board.get(mid).is_some() {
            relevant.insert(mid);
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
    let mut relevant: BTreeSet<Square> = BTreeSet::new();

    mark_jump_relevant_squares(pos, pos, &mut relevant);
    for mv in baseline {
        let hypothetical = crate::legal::apply_move_only(pos, mv);
        mark_jump_relevant_squares(&hypothetical, pos, &mut relevant);
    }
    relevant.retain(|&sq| !is_square_jump_active(pos, sq));
    relevant.into_iter().collect()
}

/// A freeze cast only changes anything if its 3x3 zone touches a square that's
/// either occupied now or reachable by one of the mover's own legal moves this
/// turn (including a castling move's rook-landing square) -- see
/// docs/superpowers/specs/2026-08-12-search-branching-factor-design.md.
pub fn relevant_freeze_targets(pos: &Position, color: Color, baseline: &[PieceMove]) -> Vec<Square> {
    if !pos.spells(color).freeze.castable() {
        return Vec::new();
    }
    let mut landing: BTreeSet<Square> = BTreeSet::new();
    for i in 0..64u8 {
        let sq = Square(i);
        if pos.board.get(sq).is_some() {
            landing.insert(sq);
        }
    }
    for mv in baseline {
        landing.insert(mv.to);
        if mv.is_castle {
            let rank = mv.from.rank();
            let rook_to = if mv.to.file() == 6 { Square::new(5, rank) } else { Square::new(3, rank) };
            landing.insert(rook_to);
        }
    }

    let mut relevant: BTreeSet<Square> = BTreeSet::new();
    for sq in landing {
        for z in freeze_zone(sq) {
            relevant.insert(z);
        }
    }
    // A square already anchoring a live freeze field is never a legal target (see
    // rules/30-freeze.md), regardless of zone geometry.
    relevant.retain(|&sq| !is_field_anchor(pos, sq, SpellKind::Freeze));
    relevant.into_iter().collect()
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
}

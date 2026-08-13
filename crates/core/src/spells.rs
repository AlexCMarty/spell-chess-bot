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

pub fn freeze_targets(pos: &Position, color: Color) -> Vec<Square> {
    if !pos.spells(color).freeze.castable() {
        return Vec::new();
    }
    (0..64).map(Square).collect()
}

pub fn is_square_jump_active(pos: &Position, square: Square) -> bool {
    pos.fields.iter().any(|f| f.kind == SpellKind::Jump && field_active(pos, f) && f.square == square)
}

pub fn jump_targets(pos: &Position, color: Color) -> Vec<Square> {
    if !pos.spells(color).jump.castable() {
        return Vec::new();
    }
    (0..64).map(Square).filter(|&sq| pos.board.get(sq).is_some()).collect()
}

/// A jump cast only changes anything if the target is the first blocker on some
/// slider's ray (either color), or a pawn's double-step mid square (either color) --
/// see docs/superpowers/specs/2026-08-12-search-branching-factor-design.md.
pub fn relevant_jump_targets(pos: &Position, color: Color) -> Vec<Square> {
    if !pos.spells(color).jump.castable() {
        return Vec::new();
    }
    let mut relevant: BTreeSet<Square> = BTreeSet::new();

    for i in 0..64u8 {
        let sq = Square(i);
        let Some(piece) = pos.board.get(sq) else { continue };
        let mut dirs: Vec<(i8, i8)> = Vec::new();
        if matches!(piece.kind, PieceKind::Rook | PieceKind::Queen) {
            dirs.extend_from_slice(&crate::rays::ROOK_DIRS);
        }
        if matches!(piece.kind, PieceKind::Bishop | PieceKind::Queen) {
            dirs.extend_from_slice(&crate::rays::BISHOP_DIRS);
        }
        for dir in dirs {
            if let Some(&blocker) = crate::rays::walk_ray(pos, sq, dir).last() {
                if pos.board.get(blocker).is_some() {
                    relevant.insert(blocker);
                }
            }
        }
    }

    for i in 0..64u8 {
        let sq = Square(i);
        let Some(piece) = pos.board.get(sq) else { continue };
        if piece.kind != PieceKind::Pawn {
            continue;
        }
        let start_rank: u8 = if piece.color == Color::White { 1 } else { 6 };
        if sq.rank() != start_rank {
            continue;
        }
        let dir: i8 = if piece.color == Color::White { 1 } else { -1 };
        let mid = Square::new(sq.file(), (sq.rank() as i8 + dir) as u8);
        if pos.board.get(mid).is_some() {
            relevant.insert(mid);
        }
    }

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
        let exhaustive = jump_targets(&pos, Color::White);
        let relevant = relevant_jump_targets(&pos, Color::White);
        for sq in &relevant {
            assert!(exhaustive.contains(sq));
        }
        assert!(relevant.len() < exhaustive.len());
    }

    #[test]
    fn f1_bishop_is_not_a_first_blocker_in_the_starting_position() {
        // f1's only slider-facing neighbors are blocked earlier: the queen's
        // rightward rank-ray stops at e1 (the king), and h1's rook's leftward
        // rank-ray stops at g1 (the knight) -- nothing ever rays as far as f1.
        let pos = Position::starting();
        let relevant = relevant_jump_targets(&pos, Color::White);
        assert!(!relevant.contains(&Square::from_str("f1").unwrap()));
    }

    #[test]
    fn b1_knight_is_a_first_blocker_in_the_starting_position() {
        // a1's rook rank-ray immediately hits b1.
        let pos = Position::starting();
        let relevant = relevant_jump_targets(&pos, Color::White);
        assert!(relevant.contains(&Square::from_str("b1").unwrap()));
    }

    #[test]
    fn relevant_jump_targets_respects_castable_gate() {
        let mut pos = Position::starting();
        pos.white_spells.jump.count = 0;
        assert!(relevant_jump_targets(&pos, Color::White).is_empty());
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

use crate::bitboard::Bitboard;
use crate::position::Position;
use crate::rays::{bishop_attacks, rook_attacks, KING_ATTACKS, KNIGHT_ATTACKS, PAWN_ATTACKS};
use crate::types::{Color, PieceKind, Square};

fn slider_occ(pos: &Position) -> Bitboard {
    pos.board.occupancy().minus(crate::spells::jump_bb(pos))
}

fn unfrozen(pos: &Position, color: Color, kind: PieceKind) -> Bitboard {
    pos.board.color_bb(color)
        .intersect(pos.board.kind_bb(kind))
        .minus(crate::spells::frozen_bb(pos))
}

fn collect_attackers(pos: &Position, square: Square, by: Color, stop_at_one: bool) -> u32 {
    let mut count = 0u32;
    let occ = slider_occ(pos);
    let idx = square.0 as usize;
    // Reverse pawn attacks: PAWN_ATTACKS[White][from] is NE/NW of from, so the white
    // pawns that attack `square` sit on PAWN_ATTACKS[Black][square], and vice versa.
    count += unfrozen(pos, by, PieceKind::Pawn)
        .intersect(PAWN_ATTACKS[by.opposite().index()][idx])
        .count();
    if stop_at_one && count > 0 {
        return count;
    }

    count += unfrozen(pos, by, PieceKind::Knight).intersect(KNIGHT_ATTACKS[idx]).count();
    if stop_at_one && count > 0 {
        return count;
    }

    count += unfrozen(pos, by, PieceKind::King).intersect(KING_ATTACKS[idx]).count();
    if stop_at_one && count > 0 {
        return count;
    }

    let bq = unfrozen(pos, by, PieceKind::Bishop).union(unfrozen(pos, by, PieceKind::Queen));
    count += bq.intersect(bishop_attacks(square, occ)).count();
    if stop_at_one && count > 0 {
        return count;
    }

    let rq = unfrozen(pos, by, PieceKind::Rook).union(unfrozen(pos, by, PieceKind::Queen));
    count += rq.intersect(rook_attacks(square, occ)).count();
    count
}

pub fn is_square_attacked(pos: &Position, square: Square, by: Color) -> bool {
    collect_attackers(pos, square, by, true) > 0
}

/// Counts distinct attackers of `square`. Only used on the rare king-capture path in
/// `legal::legal_moves`, which needs to distinguish single from double check -- a
/// plain boolean `is_square_attacked` can't, so this isn't used on the movegen hot
/// path and doesn't bother with early-exit.
pub fn attacker_count(pos: &Position, square: Square, by: Color) -> u32 {
    collect_attackers(pos, square, by, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::position::Position;
    use crate::types::{Color, Piece, PieceKind};

    #[test]
    fn rook_attacks_along_clear_file() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        assert!(is_square_attacked(&pos, Square::from_str("a8").unwrap(), Color::White));
    }

    #[test]
    fn rook_attack_blocked_by_intervening_piece() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("a4").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        assert!(!is_square_attacked(&pos, Square::from_str("a8").unwrap(), Color::White));
    }

    #[test]
    fn pawn_attacks_diagonally_forward() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("d2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        assert!(is_square_attacked(&pos, Square::from_str("e3").unwrap(), Color::White));
        assert!(!is_square_attacked(&pos, Square::from_str("d3").unwrap(), Color::White));
    }

    #[test]
    fn vector_4_frozen_piece_exerts_no_control() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("h4").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        assert!(is_square_attacked(&pos, Square::from_str("d4").unwrap(), Color::White));
        pos.fields.push(crate::position::SpellField {
            square: Square::from_str("h4").unwrap(), owner: Color::Black,
            kind: crate::position::SpellKind::Freeze, expires_after_ply: pos.ply + 1,
        });
        assert!(!is_square_attacked(&pos, Square::from_str("d4").unwrap(), Color::White));
    }

    #[test]
    fn slider_on_a_jump_square_still_attacks() {
        // Walk-from-target + last() used to see through the jumper and miss it.
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("d4").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.fields.push(crate::position::SpellField {
            square: Square::from_str("d4").unwrap(), owner: Color::White,
            kind: crate::position::SpellKind::Jump, expires_after_ply: pos.ply + 1,
        });
        assert!(is_square_attacked(&pos, Square::from_str("d8").unwrap(), Color::White));
        assert_eq!(attacker_count(&pos, Square::from_str("d8").unwrap(), Color::White), 1);
    }
}

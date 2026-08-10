use crate::position::Position;
use crate::types::{Color, Piece, PieceKind, Square};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Promotion {
    Queen,
    Rook,
    Bishop,
    Knight,
}

impl Promotion {
    pub const ALL: [Promotion; 4] = [Promotion::Queen, Promotion::Rook, Promotion::Bishop, Promotion::Knight];

    pub fn piece_kind(self) -> PieceKind {
        match self {
            Promotion::Queen => PieceKind::Queen,
            Promotion::Rook => PieceKind::Rook,
            Promotion::Bishop => PieceKind::Bishop,
            Promotion::Knight => PieceKind::Knight,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PieceMove {
    pub from: Square,
    pub to: Square,
    pub promotion: Option<Promotion>,
    pub is_en_passant: bool,
    pub is_castle: bool,
}

impl PieceMove {
    pub fn quiet(from: Square, to: Square) -> PieceMove {
        PieceMove { from, to, promotion: None, is_en_passant: false, is_castle: false }
    }
}

const KNIGHT_OFFSETS: [(i8, i8); 8] = [(1, 2), (2, 1), (2, -1), (1, -2), (-1, -2), (-2, -1), (-2, 1), (-1, 2)];
const KING_OFFSETS: [(i8, i8); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];

fn in_bounds(f: i8, r: i8) -> bool {
    (0..8).contains(&f) && (0..8).contains(&r)
}

fn leaper_moves(pos: &Position, sq: Square, offsets: &[(i8, i8)], color: Color) -> Vec<PieceMove> {
    let mut out = Vec::new();
    for (df, dr) in offsets {
        let f = sq.file() as i8 + df;
        let r = sq.rank() as i8 + dr;
        if !in_bounds(f, r) {
            continue;
        }
        let dest = Square::new(f as u8, r as u8);
        match pos.board.get(dest) {
            Some(p) if p.color == color => {}
            _ => out.push(PieceMove::quiet(sq, dest)),
        }
    }
    out
}

fn add_pawn_move(out: &mut Vec<PieceMove>, from: Square, to: Square, is_en_passant: bool, promo_rank: u8) {
    if to.rank() == promo_rank {
        for promo in Promotion::ALL {
            out.push(PieceMove { from, to, promotion: Some(promo), is_en_passant, is_castle: false });
        }
    } else {
        out.push(PieceMove { from, to, promotion: None, is_en_passant, is_castle: false });
    }
}

fn pawn_moves(pos: &Position, sq: Square, color: Color) -> Vec<PieceMove> {
    let mut out = Vec::new();
    let dir: i8 = if color == Color::White { 1 } else { -1 };
    let start_rank: i8 = if color == Color::White { 1 } else { 6 };
    let promo_rank: u8 = if color == Color::White { 7 } else { 0 };
    let (f, r) = (sq.file() as i8, sq.rank() as i8);
    let at = |rr: i8| Square::new(f as u8, rr as u8);

    // single push
    if in_bounds(f, r + dir) && pos.board.get(at(r + dir)).is_none() {
        add_pawn_move(&mut out, sq, at(r + dir), false, promo_rank);
    }

    // double push -- intermediate-square check is independent of the single-push
    // result so Task 11 can add jump-transparency here without touching this shape.
    if r == start_rank {
        let mid = at(r + dir);
        let landing = at(r + 2 * dir);
        if pos.board.get(mid).is_none() && pos.board.get(landing).is_none() {
            out.push(PieceMove::quiet(sq, landing));
        }
    }

    // captures, including en passant
    for df in [-1i8, 1i8] {
        let (cf, cr) = (f + df, r + dir);
        if !in_bounds(cf, cr) {
            continue;
        }
        let dest = Square::new(cf as u8, cr as u8);
        if let Some(p) = pos.board.get(dest) {
            if p.color != color {
                add_pawn_move(&mut out, sq, dest, false, promo_rank);
            }
        } else if pos.en_passant == Some(dest) {
            add_pawn_move(&mut out, sq, dest, true, promo_rank);
        }
    }

    out
}

fn slide_moves(pos: &Position, sq: Square, dirs: &[(i8, i8)], color: Color) -> Vec<PieceMove> {
    let mut out = Vec::new();
    for &dir in dirs {
        for dest in crate::rays::walk_ray(pos, sq, dir) {
            match pos.board.get(dest) {
                Some(p) if p.color == color => {}
                _ => out.push(PieceMove::quiet(sq, dest)),
            }
        }
    }
    out
}

fn castle_moves(pos: &Position, color: Color) -> Vec<PieceMove> {
    let mut out = Vec::new();
    let rank = if color == Color::White { 0 } else { 7 };
    let king_sq = Square::new(4, rank);
    if crate::spells::is_square_frozen(pos, king_sq) {
        return out;
    }
    if pos.board.get(king_sq) != Some(Piece { color, kind: PieceKind::King }) {
        return out;
    }
    let (kingside_right, queenside_right) = match color {
        Color::White => (pos.castle_rights.white_kingside, pos.castle_rights.white_queenside),
        Color::Black => (pos.castle_rights.black_kingside, pos.castle_rights.black_queenside),
    };
    let enemy = color.opposite();
    let attacked = |sq: Square| crate::attacks::is_square_attacked(pos, sq, enemy);
    let rook_at = |sq: Square| pos.board.get(sq) == Some(Piece { color, kind: PieceKind::Rook });

    if kingside_right
        && pos.board.get(Square::new(5, rank)).is_none()
        && pos.board.get(Square::new(6, rank)).is_none()
        && rook_at(Square::new(7, rank))
        && !crate::spells::is_square_frozen(pos, Square::new(7, rank))
        && !attacked(Square::new(4, rank)) && !attacked(Square::new(5, rank)) && !attacked(Square::new(6, rank))
    {
        out.push(PieceMove { from: king_sq, to: Square::new(6, rank), promotion: None, is_en_passant: false, is_castle: true });
    }
    if queenside_right
        && pos.board.get(Square::new(3, rank)).is_none()
        && pos.board.get(Square::new(2, rank)).is_none()
        && pos.board.get(Square::new(1, rank)).is_none()
        && rook_at(Square::new(0, rank))
        && !crate::spells::is_square_frozen(pos, Square::new(0, rank))
        && !attacked(Square::new(4, rank)) && !attacked(Square::new(3, rank)) && !attacked(Square::new(2, rank))
    {
        out.push(PieceMove { from: king_sq, to: Square::new(2, rank), promotion: None, is_en_passant: false, is_castle: true });
    }
    out
}

pub fn pseudo_legal_moves(pos: &Position) -> Vec<PieceMove> {
    let color = pos.side_to_move;
    let mut out = Vec::new();
    out.extend(castle_moves(pos, color));
    for i in 0..64 {
        let sq = Square(i);
        let piece: Piece = match pos.board.get(sq) {
            Some(p) if p.color == color => p,
            _ => continue,
        };
        if crate::spells::is_square_frozen(pos, sq) {
            continue;
        }
        match piece.kind {
            PieceKind::Pawn => out.extend(pawn_moves(pos, sq, color)),
            PieceKind::Knight => out.extend(leaper_moves(pos, sq, &KNIGHT_OFFSETS, color)),
            PieceKind::King => out.extend(leaper_moves(pos, sq, &KING_OFFSETS, color)),
            PieceKind::Bishop => out.extend(slide_moves(pos, sq, &crate::rays::BISHOP_DIRS, color)),
            PieceKind::Rook => out.extend(slide_moves(pos, sq, &crate::rays::ROOK_DIRS, color)),
            PieceKind::Queen => {
                out.extend(slide_moves(pos, sq, &crate::rays::ROOK_DIRS, color));
                out.extend(slide_moves(pos, sq, &crate::rays::BISHOP_DIRS, color));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::Position;
    use crate::types::PieceKind;

    fn moves_from(pos: &Position, sq: Square) -> Vec<Square> {
        let mut v: Vec<Square> = pseudo_legal_moves(pos).into_iter().filter(|m| m.from == sq).map(|m| m.to).collect();
        v.sort();
        v
    }

    #[test]
    fn start_position_pawn_e2_has_two_pushes() {
        let pos = Position::starting();
        let mut expected = vec![Square::from_str("e3").unwrap(), Square::from_str("e4").unwrap()];
        expected.sort();
        assert_eq!(moves_from(&pos, Square::from_str("e2").unwrap()), expected);
    }

    #[test]
    fn start_position_knight_g1_has_two_jumps() {
        let pos = Position::starting();
        let mut expected = vec![Square::from_str("f3").unwrap(), Square::from_str("h3").unwrap()];
        expected.sort();
        assert_eq!(moves_from(&pos, Square::from_str("g1").unwrap()), expected);
    }

    #[test]
    fn pawn_on_e3_cannot_double_push() {
        let mut pos = Position::starting();
        pos.board.set(Square::from_str("e2").unwrap(), None);
        pos.board.set(
            Square::from_str("e3").unwrap(),
            Some(Piece { color: Color::White, kind: PieceKind::Pawn }),
        );
        assert_eq!(moves_from(&pos, Square::from_str("e3").unwrap()), vec![Square::from_str("e4").unwrap()]);
    }

    #[test]
    fn lone_rook_on_d4_has_fourteen_moves() {
        let mut pos = Position { board: crate::board::Board::empty(), ..Position::starting() };
        pos.board.set(
            Square::from_str("d4").unwrap(),
            Some(Piece { color: Color::White, kind: PieceKind::Rook }),
        );
        assert_eq!(moves_from(&pos, Square::from_str("d4").unwrap()).len(), 14);
    }

    #[test]
    fn both_castles_available_on_clear_back_rank() {
        let mut pos = Position { board: crate::board::Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("h1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        let dests = moves_from(&pos, Square::from_str("e1").unwrap());
        assert!(dests.contains(&Square::from_str("g1").unwrap()));
        assert!(dests.contains(&Square::from_str("c1").unwrap()));
    }

    #[test]
    fn castling_blocked_when_king_passes_through_check() {
        let mut pos = Position { board: crate::board::Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("h1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("f8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        let dests = moves_from(&pos, Square::from_str("e1").unwrap());
        assert!(!dests.contains(&Square::from_str("g1").unwrap()));
    }
}

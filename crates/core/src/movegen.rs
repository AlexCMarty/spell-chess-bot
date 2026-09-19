use crate::bitboard::Bitboard;
use crate::position::Position;
use crate::rays::{bishop_attacks, rook_attacks, KING_ATTACKS, KNIGHT_ATTACKS};
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

fn in_bounds(f: i8, r: i8) -> bool {
    (0..8).contains(&f) && (0..8).contains(&r)
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

fn pawn_moves(pos: &Position, sq: Square, color: Color, jump: Bitboard) -> Vec<PieceMove> {
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

    // Double push: the intermediate square may be empty or carry a live jump field
    // (transparent). The landing square must still be empty. Independent of the
    // single-push result so a jumped blocker on `mid` still allows the double step.
    if r == start_rank {
        let mid = at(r + dir);
        let landing = at(r + 2 * dir);
        let mid_passable = pos.board.get(mid).is_none() || jump.contains(mid);
        if mid_passable && pos.board.get(landing).is_none() {
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

fn slide_dests(from: Square, own: Bitboard, slider_occ: Bitboard, bishop: bool, rook: bool) -> Vec<PieceMove> {
    let mut attacks = Bitboard::EMPTY;
    if bishop {
        attacks = attacks.union(bishop_attacks(from, slider_occ));
    }
    if rook {
        attacks = attacks.union(rook_attacks(from, slider_occ));
    }
    attacks.minus(own).iter().map(|to| PieceMove::quiet(from, to)).collect()
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
    let frozen = crate::spells::frozen_bb(pos);
    let jump = crate::spells::jump_bb(pos);
    let occ = pos.board.occupancy();
    let own = pos.board.color_bb(color);
    let slider_occ = occ.minus(jump);
    let mut out = Vec::new();
    out.extend(castle_moves(pos, color));
    for sq in own.minus(frozen).iter() {
        let piece = pos.board.get(sq).expect("color bit set");
        match piece.kind {
            PieceKind::Pawn => out.extend(pawn_moves(pos, sq, color, jump)),
            PieceKind::Knight => {
                for dest in KNIGHT_ATTACKS[sq.0 as usize].minus(own).iter() {
                    out.push(PieceMove::quiet(sq, dest));
                }
            }
            PieceKind::King => {
                for dest in KING_ATTACKS[sq.0 as usize].minus(own).iter() {
                    out.push(PieceMove::quiet(sq, dest));
                }
            }
            PieceKind::Bishop => out.extend(slide_dests(sq, own, slider_occ, true, false)),
            PieceKind::Rook => out.extend(slide_dests(sq, own, slider_occ, false, true)),
            PieceKind::Queen => out.extend(slide_dests(sq, own, slider_occ, true, true)),
        }
    }
    out
}

fn pawn_capture_moves(pos: &Position, sq: Square, color: Color, out: &mut Vec<PieceMove>) {
    let dir: i8 = if color == Color::White { 1 } else { -1 };
    let promo_rank: u8 = if color == Color::White { 7 } else { 0 };
    let (f, r) = (sq.file() as i8, sq.rank() as i8);
    for df in [-1i8, 1i8] {
        let (cf, cr) = (f + df, r + dir);
        if !in_bounds(cf, cr) {
            continue;
        }
        let dest = Square::new(cf as u8, cr as u8);
        if let Some(p) = pos.board.get(dest) {
            if p.color != color {
                add_pawn_move(out, sq, dest, false, promo_rank);
            }
        } else if pos.en_passant == Some(dest) {
            add_pawn_move(out, sq, dest, true, promo_rank);
        }
    }
}

fn slide_capture_moves(from: Square, enemy_bb: Bitboard, slider_occ: Bitboard, bishop: bool, rook: bool, out: &mut Vec<PieceMove>) {
    let mut attacks = Bitboard::EMPTY;
    if bishop {
        attacks = attacks.union(bishop_attacks(from, slider_occ));
    }
    if rook {
        attacks = attacks.union(rook_attacks(from, slider_occ));
    }
    for to in attacks.intersect(enemy_bb).iter() {
        out.push(PieceMove::quiet(from, to));
    }
}

/// Captures-only sibling of `pseudo_legal_moves`. Mirrors its exact per-square
/// iteration order (`own.minus(frozen)`, ascending) and per-`PieceKind` dispatch, so
/// filtering `pseudo_legal_moves`'s output down to captures yields precisely this
/// function's output, in the same relative order -- see
/// `pseudo_legal_captures_matches_pseudo_legal_moves_filtered_ordered` below. Pushes
/// directly into one shared `out` (no per-piece `Vec` allocation via `pawn_moves`/
/// `slide_dests`, unlike `pseudo_legal_moves`) since the saving this exists for is as
/// much about allocation count as element count. Castling is never a capture, so
/// `castle_moves` is never called here.
pub fn pseudo_legal_captures(pos: &Position) -> Vec<PieceMove> {
    let color = pos.side_to_move;
    let frozen = crate::spells::frozen_bb(pos);
    let jump = crate::spells::jump_bb(pos);
    let occ = pos.board.occupancy();
    let own = pos.board.color_bb(color);
    let enemy_bb = pos.board.color_bb(color.opposite());
    let slider_occ = occ.minus(jump);
    let mut out = Vec::new();
    for sq in own.minus(frozen).iter() {
        let piece = pos.board.get(sq).expect("color bit set");
        match piece.kind {
            PieceKind::Pawn => pawn_capture_moves(pos, sq, color, &mut out),
            PieceKind::Knight => {
                for dest in KNIGHT_ATTACKS[sq.0 as usize].intersect(enemy_bb).iter() {
                    out.push(PieceMove::quiet(sq, dest));
                }
            }
            PieceKind::King => {
                for dest in KING_ATTACKS[sq.0 as usize].intersect(enemy_bb).iter() {
                    out.push(PieceMove::quiet(sq, dest));
                }
            }
            PieceKind::Bishop => slide_capture_moves(sq, enemy_bb, slider_occ, true, false, &mut out),
            PieceKind::Rook => slide_capture_moves(sq, enemy_bb, slider_occ, false, true, &mut out),
            PieceKind::Queen => slide_capture_moves(sq, enemy_bb, slider_occ, true, true, &mut out),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
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

    fn captures_from(pos: &Position, sq: Square) -> Vec<Square> {
        let mut v: Vec<Square> = pseudo_legal_captures(pos).into_iter().filter(|m| m.from == sq).map(|m| m.to).collect();
        v.sort();
        v
    }

    #[test]
    fn pawn_captures_diagonally_but_not_by_pushing() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e4").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("d5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("f5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        let mut expected = vec![Square::from_str("d5").unwrap(), Square::from_str("f5").unwrap()];
        expected.sort();
        assert_eq!(captures_from(&pos, Square::from_str("e4").unwrap()), expected);
    }

    #[test]
    fn pawn_en_passant_capture_is_included() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e5").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("d5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.en_passant = Some(Square::from_str("d6").unwrap());
        let caps = pseudo_legal_captures(&pos);
        let found = caps.iter().find(|m| m.from == Square::from_str("e5").unwrap() && m.to == Square::from_str("d6").unwrap());
        assert!(found.is_some_and(|m| m.is_en_passant), "en passant capture must be included and flagged");
    }

    #[test]
    fn pawn_capture_promotion_included_quiet_promotion_excluded() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("b7").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("a8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        let caps = pseudo_legal_captures(&pos);
        let b7 = Square::from_str("b7").unwrap();
        let a8 = Square::from_str("a8").unwrap();
        let b8 = Square::from_str("b8").unwrap();
        assert_eq!(
            caps.iter().filter(|m| m.from == b7 && m.to == a8).count(), 4,
            "all four promotion pieces must appear for the capture on a8",
        );
        assert!(!caps.iter().any(|m| m.from == b7 && m.to == b8), "quiet promotion to b8 (no capture) must not appear");
    }

    #[test]
    fn knight_captures_only_enemy_occupied_squares() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("d4").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Knight }));
        pos.board.set(Square::from_str("c6").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("e6").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        assert_eq!(captures_from(&pos, Square::from_str("d4").unwrap()), vec![Square::from_str("c6").unwrap()]);
    }

    #[test]
    fn king_captures_only_enemy_occupied_squares() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e4").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("e5").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        assert_eq!(captures_from(&pos, Square::from_str("e4").unwrap()), vec![Square::from_str("d5").unwrap()]);
    }

    #[test]
    fn slider_captures_through_a_live_jump_square() {
        use crate::position::{SpellField, SpellKind};
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("d1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("d4").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("d8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        assert!(captures_from(&pos, Square::from_str("d1").unwrap()).is_empty(), "fixture is wrong: d1 must not see d8 before the jump");
        pos.fields.push(SpellField {
            square: Square::from_str("d4").unwrap(),
            owner: Color::White,
            kind: SpellKind::Jump,
            expires_after_ply: pos.ply + 1,
        });
        assert_eq!(captures_from(&pos, Square::from_str("d1").unwrap()), vec![Square::from_str("d8").unwrap()]);
    }

    #[test]
    fn castling_never_appears_as_a_capture() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("h1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        assert!(pseudo_legal_moves(&pos).iter().any(|m| m.is_castle), "fixture is wrong: castling must be pseudo-legal here");
        assert!(!pseudo_legal_captures(&pos).iter().any(|m| m.is_castle));
    }

    #[test]
    fn pseudo_legal_captures_matches_pseudo_legal_moves_filtered_ordered() {
        fn is_cap(pos: &Position, mv: &PieceMove) -> bool {
            pos.board.get(mv.to).is_some() || mv.is_en_passant
        }
        let mut battery = vec![Position::starting()];
        let mut sparse = Position { board: Board::empty(), ..Position::starting() };
        sparse.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        sparse.board.set(Square::from_str("d1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        sparse.board.set(Square::from_str("d4").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        sparse.board.set(Square::from_str("d8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        sparse.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        battery.push(sparse);

        // A near-promotion pawn with an enemy piece on *both* capture diagonals. Without
        // it this battery never reached a capture at all -- `Position::starting()` has
        // none, and `sparse`'s rook is blocked by its own pawn on d4 (that fixture is the
        // jump fixture *without* the jump field) -- so the `assert_eq!` below compared two
        // empty vectors and passed on any capture-ordering bug. Both diagonals are load
        // bearing: they pin the `[-1, 1]` loop order in `pawn_capture_moves`, and landing
        // on rank 8 pins the four-promotion burst `add_pawn_move` emits, which is the
        // finest-grained ordering either generator produces and the only one no other
        // fixture reaches. Reversing either order is otherwise invisible to the whole
        // workspace suite, and it is not cosmetic: quiescence does not sort (neither
        // `dedup_captures` nor its loop), so emission order is try order and a change here
        // moves qnode counts.
        let mut promo = Position { board: Board::empty(), ..Position::starting() };
        promo.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        promo.board.set(Square::from_str("b7").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        promo.board.set(Square::from_str("a8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        promo.board.set(Square::from_str("c8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Knight }));
        promo.board.set(Square::from_str("h8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        battery.push(promo);

        let mut captures = 0usize;
        let mut capture_promotions = 0usize;
        for pos in battery {
            let expected: Vec<PieceMove> = pseudo_legal_moves(&pos).into_iter().filter(|m| is_cap(&pos, m)).collect();
            let actual = pseudo_legal_captures(&pos);
            assert_eq!(actual, expected, "order/set mismatch for side_to_move {:?}", pos.side_to_move);
            captures += expected.len();
            capture_promotions += expected.iter().filter(|m| m.promotion.is_some()).count();
        }

        // Vacuity guard. Counted off `expected` (the `pseudo_legal_moves` oracle), not off
        // `actual`, so a generator that silently stopped emitting captures fails the
        // `assert_eq!` above rather than quietly weakening these bounds. This is the check
        // whose absence let the battery sit vacuous; keep it if the fixtures are edited.
        assert!(captures > 0, "fixtures are wrong: the battery must reach at least one capture");
        assert_eq!(
            capture_promotions, 8,
            "fixtures are wrong: both capture diagonals must offer all four promotion pieces",
        );
    }
}

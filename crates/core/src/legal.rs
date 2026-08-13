use crate::position::{Position, SpellField, SpellKind};
use crate::movegen::{pseudo_legal_moves, PieceMove};
use crate::types::{Color, Piece, PieceKind, Square};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpellCast {
    pub kind: SpellKind,
    pub square: Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Turn {
    pub spell: Option<SpellCast>,
    pub mv: PieceMove,
}

pub fn apply_move_only(pos: &Position, mv: &PieceMove) -> Position {
    let mut next = pos.clone();
    let mover = pos.board.get(mv.from).expect("apply_move_only: no piece on from-square");

    if mv.is_en_passant {
        let captured_sq = Square::new(mv.to.file(), mv.from.rank());
        next.board.set(captured_sq, None);
    }
    if mv.is_castle {
        let rank = mv.from.rank();
        let (rook_from, rook_to) = if mv.to.file() == 6 {
            (Square::new(7, rank), Square::new(5, rank))
        } else {
            (Square::new(0, rank), Square::new(3, rank))
        };
        let rook = next.board.get(rook_from).expect("apply_move_only: castling rook missing");
        next.board.set(rook_from, None);
        next.board.set(rook_to, Some(rook));
    }

    let is_capture = pos.board.get(mv.to).is_some() || mv.is_en_passant;
    next.board.set(mv.from, None);
    let placed = match mv.promotion {
        Some(promo) => Piece { color: mover.color, kind: promo.piece_kind() },
        None => mover,
    };
    next.board.set(mv.to, Some(placed));

    if mover.kind == PieceKind::King {
        match mover.color {
            Color::White => { next.castle_rights.white_kingside = false; next.castle_rights.white_queenside = false; }
            Color::Black => { next.castle_rights.black_kingside = false; next.castle_rights.black_queenside = false; }
        }
    }
    let touched = [mv.from, mv.to];
    if touched.contains(&Square::new(0, 0)) { next.castle_rights.white_queenside = false; }
    if touched.contains(&Square::new(7, 0)) { next.castle_rights.white_kingside = false; }
    if touched.contains(&Square::new(0, 7)) { next.castle_rights.black_queenside = false; }
    if touched.contains(&Square::new(7, 7)) { next.castle_rights.black_kingside = false; }

    next.en_passant = if mover.kind == PieceKind::Pawn && mv.from.rank().abs_diff(mv.to.rank()) == 2 {
        Some(Square::new(mv.from.file(), (mv.from.rank() + mv.to.rank()) / 2))
    } else {
        None
    };

    next.halfmove_clock = if mover.kind == PieceKind::Pawn || is_capture { 0 } else { pos.halfmove_clock + 1 };
    next
}

pub fn legal_moves(pos: &Position) -> Vec<PieceMove> {
    let mover = pos.side_to_move;
    pseudo_legal_moves(pos)
        .into_iter()
        .filter(|mv| {
            let after = apply_move_only(pos, mv);
            if after.board.king_square(mover.opposite()).is_none() {
                // This move captures the enemy king outright, ending the game
                // immediately -- so it's legal even if the mover's own king is left
                // in check (single, double, or otherwise -- a pre-existing double
                // check doesn't block it either, see rules/50-interactions.md
                // #win-conditions), UNLESS *this exact move* is what pushes the
                // mover's own checker count higher than it already was and past 1 --
                // i.e. the capturing piece was itself blocking a different attacker,
                // and unpinning it to reach the enemy king discovers a second,
                // simultaneous check that didn't already exist. Orthodox rules
                // require a king move to answer double check; that survives the
                // king-capture exemption only in the "you just created this problem
                // yourself" case, not the "you already had this problem" case.
                let before_sq = pos.board.king_square(mover).expect("mover's king must be on the board before its own move");
                let after_sq = after.board.king_square(mover).expect("mover's own king must still be on the board");
                let before_count = crate::attacks::attacker_count(pos, before_sq, mover.opposite());
                let after_count = crate::attacks::attacker_count(&after, after_sq, mover.opposite());
                return after_count <= before_count.max(1);
            }
            match after.board.king_square(mover) {
                Some(king_sq) => !crate::attacks::is_square_attacked(&after, king_sq, mover.opposite()),
                None => true, // this move itself captured the enemy king on a prior ply; not reachable here
            }
        })
        .collect()
}

fn position_with_field(pos: &Position, cast: SpellCast) -> Position {
    let mut next = pos.clone();
    next.fields.push(SpellField {
        square: cast.square,
        owner: pos.side_to_move,
        kind: cast.kind,
        expires_after_ply: pos.ply + 1,
    });
    next
}

fn generate_turns_from(
    pos: &Position,
    baseline: Vec<PieceMove>,
    freeze_targets: Vec<Square>,
    jump_targets: Vec<Square>,
) -> Vec<Turn> {
    let mut turns: Vec<Turn> = baseline.into_iter().map(|mv| Turn { spell: None, mv }).collect();

    for sq in freeze_targets {
        let cast = SpellCast { kind: SpellKind::Freeze, square: sq };
        let hypothetical = position_with_field(pos, cast);
        turns.extend(legal_moves(&hypothetical).into_iter().map(|mv| Turn { spell: Some(cast), mv }));
    }
    for sq in jump_targets {
        let cast = SpellCast { kind: SpellKind::Jump, square: sq };
        let hypothetical = position_with_field(pos, cast);
        turns.extend(legal_moves(&hypothetical).into_iter().map(|mv| Turn { spell: Some(cast), mv }));
    }
    turns
}

pub fn generate_turns(pos: &Position) -> Vec<Turn> {
    let color = pos.side_to_move;
    generate_turns_from(
        pos,
        legal_moves(pos),
        crate::spells::freeze_targets(pos, color),
        crate::spells::jump_targets(pos, color),
    )
}

pub fn generate_search_turns(pos: &Position) -> Vec<Turn> {
    let color = pos.side_to_move;
    let baseline = legal_moves(pos);
    let freeze_targets = crate::spells::relevant_freeze_targets(pos, color, &baseline);
    let jump_targets = crate::spells::relevant_jump_targets(pos, color, &baseline);
    generate_turns_from(pos, baseline, freeze_targets, jump_targets)
}

pub fn apply_turn(pos: &Position, turn: &Turn) -> Position {
    let mover = pos.side_to_move;
    let mut next = apply_move_only(pos, &turn.mv);

    if let Some(cast) = turn.spell {
        next.fields.push(SpellField {
            square: cast.square, owner: mover, kind: cast.kind, expires_after_ply: pos.ply + 1,
        });
        let counter = match (cast.kind, mover) {
            (SpellKind::Freeze, Color::White) => &mut next.white_spells.freeze,
            (SpellKind::Freeze, Color::Black) => &mut next.black_spells.freeze,
            (SpellKind::Jump, Color::White) => &mut next.white_spells.jump,
            (SpellKind::Jump, Color::Black) => &mut next.black_spells.jump,
        };
        counter.count -= 1;
        counter.lock = 3;
    }

    // A side's lock decrements once per turn its opponent completes (rules/20-spell-system.md#cooldown-timing).
    let opponent_spells = match mover {
        Color::White => &mut next.black_spells,
        Color::Black => &mut next.white_spells,
    };
    if opponent_spells.freeze.lock > 0 { opponent_spells.freeze.lock -= 1; }
    if opponent_spells.jump.lock > 0 { opponent_spells.jump.lock -= 1; }

    next.ply = pos.ply + 1;
    next.fields.retain(|f| next.ply <= f.expires_after_ply);
    next.side_to_move = mover.opposite();
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::position::{Position, SpellCounter, CastleRights};
    use crate::types::{Color, PieceKind};

    fn dests(pos: &Position, sq: Square) -> Vec<Square> {
        let mut v: Vec<Square> = legal_moves(pos).into_iter().filter(|m| m.from == sq).map(|m| m.to).collect();
        v.sort();
        v.dedup();
        v
    }

    #[test]
    fn vector_1_start_position_sanity() {
        let pos = Position::starting();
        assert_eq!(dests(&pos, Square::from_str("e2").unwrap()), vec![Square::from_str("e3").unwrap(), Square::from_str("e4").unwrap()]);
        assert_eq!(dests(&pos, Square::from_str("g1").unwrap()), vec![Square::from_str("f3").unwrap(), Square::from_str("h3").unwrap()]);
    }

    #[test]
    fn vector_17_promotion() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("b7").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("h8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        assert_eq!(dests(&pos, Square::from_str("b7").unwrap()), vec![Square::from_str("b8").unwrap()]);
    }

    #[test]
    fn king_cannot_move_into_check() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a2").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        assert!(!dests(&pos, Square::from_str("e1").unwrap()).contains(&Square::from_str("e2").unwrap()));
    }

    #[test]
    fn vector_2_freeze_immobilizes_the_targeted_piece() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("h7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Knight }));
        pos.side_to_move = Color::Black;
        pos.fields.push(crate::position::SpellField {
            square: Square::from_str("d5").unwrap(), owner: Color::White,
            kind: crate::position::SpellKind::Freeze, expires_after_ply: pos.ply + 1,
        });
        assert_eq!(dests(&pos, Square::from_str("d5").unwrap()), Vec::<Square>::new());
    }

    #[test]
    fn vector_7_own_freeze_binds_own_piece_same_turn() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("d4").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Knight }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.fields.push(crate::position::SpellField {
            square: Square::from_str("d4").unwrap(), owner: Color::White,
            kind: crate::position::SpellKind::Freeze, expires_after_ply: pos.ply + 1,
        });
        assert!(dests(&pos, Square::from_str("d4").unwrap()).is_empty());
        assert!(!dests(&pos, Square::from_str("a1").unwrap()).is_empty());
    }

    #[test]
    fn vector_6_frozen_piece_still_blocks_and_is_capturable() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Knight }));
        pos.fields.push(crate::position::SpellField {
            square: Square::from_str("d5").unwrap(), owner: Color::Black,
            kind: crate::position::SpellKind::Freeze, expires_after_ply: pos.ply + 1,
        });
        let rook_dests = dests(&pos, Square::from_str("d1").unwrap());
        assert!(rook_dests.contains(&Square::from_str("d5").unwrap()));
        assert!(!rook_dests.contains(&Square::from_str("d6").unwrap()));
    }

    #[test]
    fn vector_5_freezing_the_checker_dispels_check() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d8").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("h8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        pos.side_to_move = Color::Black;
        assert!(crate::attacks::is_square_attacked(&pos, Square::from_str("e8").unwrap(), Color::White));
        pos.fields.push(crate::position::SpellField {
            square: Square::from_str("d8").unwrap(), owner: Color::Black,
            kind: crate::position::SpellKind::Freeze, expires_after_ply: pos.ply + 1,
        });
        let h8_dests = dests(&pos, Square::from_str("h8").unwrap());
        assert!(h8_dests.contains(&Square::from_str("h7").unwrap()));
    }

    #[test]
    fn vector_10_jump_field_serves_both_players() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("d4").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        pos.fields.push(crate::position::SpellField {
            square: Square::from_str("d4").unwrap(), owner: Color::White,
            kind: crate::position::SpellKind::Jump, expires_after_ply: pos.ply + 1,
        });
        assert!(dests(&pos, Square::from_str("d1").unwrap()).contains(&Square::from_str("d8").unwrap()));
        let mut black_pos = pos.clone();
        black_pos.side_to_move = Color::Black;
        assert!(dests(&black_pos, Square::from_str("d8").unwrap()).contains(&Square::from_str("d1").unwrap()));
    }

    #[test]
    fn vector_11_pawn_double_steps_over_jumped_blocker() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("d3").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        assert!(dests(&pos, Square::from_str("d2").unwrap()).is_empty());
        pos.fields.push(crate::position::SpellField {
            square: Square::from_str("d3").unwrap(), owner: Color::White,
            kind: crate::position::SpellKind::Jump, expires_after_ply: pos.ply + 1,
        });
        assert_eq!(dests(&pos, Square::from_str("d2").unwrap()), vec![Square::from_str("d4").unwrap()]);
    }

    #[test]
    fn vector_9_king_capture_via_jump() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Bishop }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("h2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("b4").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Bishop }));
        pos.board.set(Square::from_str("a8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        pos.side_to_move = Color::Black;
        pos.fields.push(crate::position::SpellField {
            square: Square::from_str("d2").unwrap(), owner: Color::Black,
            kind: crate::position::SpellKind::Jump, expires_after_ply: pos.ply + 1,
        });
        let capture = PieceMove::quiet(Square::from_str("b4").unwrap(), Square::from_str("e1").unwrap());
        assert!(legal_moves(&pos).contains(&capture));
        let after = apply_move_only(&pos, &capture);
        assert_eq!(after.board.king_square(Color::White), None);
    }

    #[test]
    fn vector_12_check_through_jump_square_is_unblockable() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Bishop }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("h2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("b4").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Bishop }));
        pos.fields.push(crate::position::SpellField {
            square: Square::from_str("d2").unwrap(), owner: Color::Black,
            kind: crate::position::SpellKind::Jump, expires_after_ply: pos.ply + 1,
        });
        assert!(dests(&pos, Square::from_str("a1").unwrap()).is_empty());
        assert!(dests(&pos, Square::from_str("d2").unwrap()).contains(&Square::from_str("b4").unwrap()));
        let king_dests = dests(&pos, Square::from_str("e1").unwrap());
        assert!(!king_dests.is_empty());
    }

    #[test]
    fn generate_turns_includes_no_spell_and_spell_options_at_start() {
        let pos = Position::starting();
        let turns = generate_turns(&pos);
        let no_spell_count = turns.iter().filter(|t| t.spell.is_none()).count();
        assert_eq!(no_spell_count, legal_moves(&pos).len());
        assert!(turns.iter().any(|t| matches!(t.spell, Some(SpellCast { kind: SpellKind::Freeze, .. }))));
        assert!(turns.iter().any(|t| matches!(t.spell, Some(SpellCast { kind: SpellKind::Jump, .. }))));
    }

    #[test]
    fn vector_19_illegal_casts_are_never_offered() {
        let mut pos = Position::starting();
        pos.white_spells.jump.count = 0;
        pos.white_spells.freeze.lock = 2;
        let turns = generate_turns(&pos);
        assert!(!turns.iter().any(|t| t.spell.is_some()));
    }

    #[test]
    fn generate_search_turns_matches_generate_turns_move_count_on_a_no_spell_position() {
        let mut pos = Position::starting();
        pos.white_spells.freeze.count = 0;
        pos.white_spells.jump.count = 0;
        let exhaustive = generate_turns(&pos);
        let filtered = generate_search_turns(&pos);
        assert_eq!(exhaustive.len(), filtered.len());
        assert_eq!(exhaustive.len(), legal_moves(&pos).len());
    }

    #[test]
    fn generate_search_turns_is_never_larger_than_generate_turns() {
        let pos = Position::starting();
        assert!(generate_search_turns(&pos).len() <= generate_turns(&pos).len());
    }

    #[test]
    fn vector_18_cooldown_timeline() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));

        let cast = SpellCast { kind: SpellKind::Freeze, square: Square::from_str("d5").unwrap() };
        let mv = PieceMove::quiet(Square::from_str("a1").unwrap(), Square::from_str("a2").unwrap());
        pos = apply_turn(&pos, &Turn { spell: Some(cast), mv });
        assert_eq!(pos.spells(Color::White).freeze, SpellCounter { count: 4, lock: 3 });
        assert!(pos.fields.is_empty() == false);

        let quiet = |_pos: &Position, from: &str, to: &str| Turn {
            spell: None,
            mv: PieceMove::quiet(Square::from_str(from).unwrap(), Square::from_str(to).unwrap()),
        };

        pos = apply_turn(&pos, &quiet(&pos, "a8", "a7")); // Black's reply
        assert_eq!(pos.spells(Color::White).freeze, SpellCounter { count: 4, lock: 2 });
        assert!(pos.fields.is_empty());

        pos = apply_turn(&pos, &quiet(&pos, "a2", "a3")); // White N+1
        assert_eq!(pos.spells(Color::White).freeze, SpellCounter { count: 4, lock: 2 });
        pos = apply_turn(&pos, &quiet(&pos, "a7", "a6")); // Black reply
        assert_eq!(pos.spells(Color::White).freeze, SpellCounter { count: 4, lock: 1 });
        pos = apply_turn(&pos, &quiet(&pos, "a3", "a4")); // White N+2
        assert_eq!(pos.spells(Color::White).freeze, SpellCounter { count: 4, lock: 1 });
        pos = apply_turn(&pos, &quiet(&pos, "a6", "a5")); // Black reply
        assert_eq!(pos.spells(Color::White).freeze, SpellCounter { count: 4, lock: 0 });
        assert!(pos.spells(Color::White).freeze.castable());
    }

    #[test]
    fn vector_15_castling_under_freeze() {
        // Position: {e1:'0K', h1:'0R', a1:'0R', e8:'2K', h8:'2R', a8:'2R'}, all castling rights on.
        let mut pos = Position { board: Board::empty(), castle_rights: CastleRights::all(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("h1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("h8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("a8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));

        // Baseline: both castles available
        assert!(dests(&pos, Square::from_str("e1").unwrap()).contains(&Square::from_str("g1").unwrap()));
        assert!(dests(&pos, Square::from_str("e1").unwrap()).contains(&Square::from_str("c1").unwrap()));

        // freeze@c1 (king's landing square for queenside castle) → no effect
        let mut pos_c1 = pos.clone();
        pos_c1.fields.push(crate::position::SpellField {
            square: Square::from_str("c1").unwrap(), owner: Color::Black,
            kind: crate::position::SpellKind::Freeze, expires_after_ply: pos_c1.ply + 1,
        });
        assert!(dests(&pos_c1, Square::from_str("e1").unwrap()).contains(&Square::from_str("g1").unwrap()));
        assert!(dests(&pos_c1, Square::from_str("e1").unwrap()).contains(&Square::from_str("c1").unwrap()));

        // freeze@a1 (queenside rook) → queenside lost, kingside available
        let mut pos_a1 = pos.clone();
        pos_a1.fields.push(crate::position::SpellField {
            square: Square::from_str("a1").unwrap(), owner: Color::Black,
            kind: crate::position::SpellKind::Freeze, expires_after_ply: pos_a1.ply + 1,
        });
        assert!(dests(&pos_a1, Square::from_str("e1").unwrap()).contains(&Square::from_str("g1").unwrap()));
        assert!(!dests(&pos_a1, Square::from_str("e1").unwrap()).contains(&Square::from_str("c1").unwrap()));

        // freeze@g1 (kingside rook square) → kingside lost, queenside available
        let mut pos_g1 = pos.clone();
        pos_g1.fields.push(crate::position::SpellField {
            square: Square::from_str("g1").unwrap(), owner: Color::Black,
            kind: crate::position::SpellKind::Freeze, expires_after_ply: pos_g1.ply + 1,
        });
        assert!(!dests(&pos_g1, Square::from_str("e1").unwrap()).contains(&Square::from_str("g1").unwrap()));
        assert!(dests(&pos_g1, Square::from_str("e1").unwrap()).contains(&Square::from_str("c1").unwrap()));
    }
}

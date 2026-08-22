use spellchess_core::*;

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

fn random_legal_walk(seed: u64, plies: u32) -> Vec<Position> {
    let mut state = seed;
    let mut pos = Position::starting();
    let mut out = vec![pos.clone()];
    for _ in 0..plies {
        let turns = generate_turns(&pos);
        if turns.is_empty() {
            break;
        }
        let pick = (splitmix64(&mut state) as usize) % turns.len();
        pos = apply_turn(&pos, &turns[pick]);
        if pos.board.king_square(Color::White).is_none() || pos.board.king_square(Color::Black).is_none() {
            break;
        }
        out.push(pos.clone());
    }
    out
}

fn sparse_endgame() -> Position {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(Square::from_str("h8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
    pos
}

fn battery() -> Vec<Position> {
    let mut out = vec![Position::starting(), sparse_endgame()];
    out.extend(random_legal_walk(1, 8));
    out.extend(random_legal_walk(2, 8));
    out.extend(random_legal_walk(7, 12));
    out.extend(random_legal_walk(99, 12));
    out
}

fn with_field(pos: &Position, cast: SpellCast) -> Position {
    let mut next = pos.clone();
    next.fields.push(SpellField {
        square: cast.square,
        owner: pos.side_to_move,
        kind: cast.kind,
        expires_after_ply: pos.ply + 1,
    });
    next
}

fn sort_key(mv: &PieceMove) -> (u8, u8, u8, bool, bool) {
    (mv.from.0, mv.to.0, mv.promotion.map(|p| p as u8).unwrap_or(255), mv.is_en_passant, mv.is_castle)
}

fn sorted(moves: &[PieceMove]) -> Vec<(u8, u8, u8, bool, bool)> {
    let mut v: Vec<_> = moves.iter().map(sort_key).collect();
    v.sort();
    v
}

#[test]
fn relevant_freeze_targets_is_always_a_subset() {
    for pos in battery() {
        for &color in &[Color::White, Color::Black] {
            let baseline = legal_moves(&pos);
            let relevant = spells::relevant_freeze_targets(&pos, color, &baseline);
            let exhaustive = spells::freeze_targets(&pos, color);
            for sq in relevant {
                assert!(exhaustive.contains(&sq), "relevant freeze target {sq} missing from exhaustive set");
            }
        }
    }
}

#[test]
fn relevant_jump_targets_is_always_a_subset() {
    for pos in battery() {
        for &color in &[Color::White, Color::Black] {
            let baseline = legal_moves(&pos);
            let relevant = spells::relevant_jump_targets(&pos, color, &baseline);
            let exhaustive = spells::jump_targets(&pos, color);
            for sq in relevant {
                assert!(exhaustive.contains(&sq), "relevant jump target {sq} missing from exhaustive set");
            }
        }
    }
}

#[test]
fn excluded_freeze_targets_do_not_change_this_ply_legal_moves() {
    for pos in battery() {
        let color = pos.side_to_move;
        let baseline_moves = legal_moves(&pos);
        let relevant = spells::relevant_freeze_targets(&pos, color, &baseline_moves);
        let exhaustive = spells::freeze_targets(&pos, color);
        for sq in exhaustive {
            if relevant.contains(&sq) {
                continue;
            }
            let cast = SpellCast { kind: SpellKind::Freeze, square: sq };
            let hypothetical = with_field(&pos, cast);
            assert_eq!(
                sorted(&legal_moves(&hypothetical)), sorted(&baseline_moves),
                "excluded freeze target {sq} changed this-ply legal moves"
            );
        }
    }
}

#[test]
fn excluded_freeze_targets_do_not_change_next_ply_legal_moves() {
    for pos in battery() {
        let color = pos.side_to_move;
        let baseline_moves = legal_moves(&pos);
        let relevant = spells::relevant_freeze_targets(&pos, color, &baseline_moves);
        let exhaustive = spells::freeze_targets(&pos, color);
        for sq in exhaustive {
            if relevant.contains(&sq) {
                continue;
            }
            let cast = SpellCast { kind: SpellKind::Freeze, square: sq };
            for mv in &baseline_moves {
                let with_spell = apply_turn(&pos, &Turn { spell: Some(cast), mv: *mv });
                let without_spell = apply_turn(&pos, &Turn { spell: None, mv: *mv });
                assert_eq!(
                    sorted(&legal_moves(&with_spell)), sorted(&legal_moves(&without_spell)),
                    "excluded freeze target {sq} changed next-ply legal moves after {mv:?}"
                );
            }
        }
    }
}

#[test]
fn excluded_jump_targets_do_not_change_this_ply_legal_moves() {
    for pos in battery() {
        let color = pos.side_to_move;
        let baseline_moves = legal_moves(&pos);
        let relevant = spells::relevant_jump_targets(&pos, color, &baseline_moves);
        let exhaustive = spells::jump_targets(&pos, color);
        for sq in exhaustive {
            if relevant.contains(&sq) {
                continue;
            }
            let cast = SpellCast { kind: SpellKind::Jump, square: sq };
            let hypothetical = with_field(&pos, cast);
            assert_eq!(
                sorted(&legal_moves(&hypothetical)), sorted(&baseline_moves),
                "excluded jump target {sq} changed this-ply legal moves"
            );
        }
    }
}

#[test]
fn excluded_jump_targets_do_not_change_next_ply_legal_moves() {
    for pos in battery() {
        let color = pos.side_to_move;
        let baseline_moves = legal_moves(&pos);
        let relevant = spells::relevant_jump_targets(&pos, color, &baseline_moves);
        let exhaustive = spells::jump_targets(&pos, color);
        for sq in exhaustive {
            if relevant.contains(&sq) {
                continue;
            }
            let cast = SpellCast { kind: SpellKind::Jump, square: sq };
            for mv in &baseline_moves {
                let with_spell = apply_turn(&pos, &Turn { spell: Some(cast), mv: *mv });
                let without_spell = apply_turn(&pos, &Turn { spell: None, mv: *mv });
                assert_eq!(
                    sorted(&legal_moves(&with_spell)), sorted(&legal_moves(&without_spell)),
                    "excluded jump target {sq} changed next-ply legal moves after {mv:?}"
                );
            }
        }
    }
}

#[test]
fn generate_turns_spell_moves_match_legal_moves_on_hypothetical() {
    for pos in battery() {
        let turns = generate_turns(&pos);
        let color = pos.side_to_move;
        for sq in spells::freeze_targets(&pos, color) {
            let cast = SpellCast { kind: SpellKind::Freeze, square: sq };
            let from_turns: Vec<_> = turns.iter().filter(|t| t.spell == Some(cast)).map(|t| t.mv).collect();
            let hypothetical = with_field(&pos, cast);
            assert_eq!(
                sorted(&from_turns), sorted(&legal_moves(&hypothetical)),
                "freeze@{sq} this-ply moves diverged from legal_moves(hypothetical)"
            );
        }
        for sq in spells::jump_targets(&pos, color) {
            let cast = SpellCast { kind: SpellKind::Jump, square: sq };
            let from_turns: Vec<_> = turns.iter().filter(|t| t.spell == Some(cast)).map(|t| t.mv).collect();
            let hypothetical = with_field(&pos, cast);
            assert_eq!(
                sorted(&from_turns), sorted(&legal_moves(&hypothetical)),
                "jump@{sq} this-ply moves diverged from legal_moves(hypothetical)"
            );
        }
    }
}

#[test]
fn generate_search_turns_skips_only_next_ply_inert_baseline_spell_turns() {
    for pos in battery() {
        let exhaustive = generate_turns(&pos);
        let filtered = generate_search_turns(&pos);
        for turn in exhaustive {
            if filtered.contains(&turn) {
                continue;
            }
            let Some(cast) = turn.spell else {
                panic!("search omitted a no-spell turn {turn:?}");
            };
            assert!(
                generate_turns(&pos).iter().any(|t| t.spell.is_none() && t.mv == turn.mv),
                "omitted spell turn {turn:?} has no no-spell counterpart"
            );
            let with_spell = apply_turn(&pos, &turn);
            let without = apply_turn(&pos, &Turn { spell: None, mv: turn.mv });
            assert_eq!(
                sorted(&legal_moves(&with_spell)), sorted(&legal_moves(&without)),
                "omitted {cast:?} + {:?} changed next-ply legal moves", turn.mv
            );
        }
    }
}

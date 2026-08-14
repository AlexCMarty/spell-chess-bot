use spellchess_core::*;

fn legal_moves_reference(pos: &Position) -> Vec<PieceMove> {
    let mover = pos.side_to_move;
    pseudo_legal_moves(pos)
        .into_iter()
        .filter(|mv| {
            let after = apply_move_only(pos, mv);
            if after.board.king_square(mover.opposite()).is_none() {
                let before_sq = pos.board.king_square(mover).expect("mover's king");
                let after_sq = after.board.king_square(mover).expect("mover's king after");
                let before_count = attacker_count(pos, before_sq, mover.opposite());
                let after_count = attacker_count(&after, after_sq, mover.opposite());
                return after_count <= before_count.max(1);
            }
            match after.board.king_square(mover) {
                Some(king_sq) => !is_square_attacked(&after, king_sq, mover.opposite()),
                None => true,
            }
        })
        .collect()
}

fn sort_key(mv: &PieceMove) -> (u8, u8, u8, bool, bool) {
    (
        mv.from.0,
        mv.to.0,
        mv.promotion.map(|p| p as u8).unwrap_or(255),
        mv.is_en_passant,
        mv.is_castle,
    )
}

fn sorted(moves: &[PieceMove]) -> Vec<(u8, u8, u8, bool, bool)> {
    let mut v: Vec<_> = moves.iter().map(sort_key).collect();
    v.sort();
    v
}

#[test]
fn production_matches_reference_on_the_starting_position() {
    let pos = Position::starting();
    assert_eq!(
        sorted(&legal_moves(&pos)),
        sorted(&legal_moves_reference(&pos))
    );
}

/// A pinned rook capturing the jumped piece on its pin ray lands on the jump
/// square and stays transparent, so the pinner still checks the king. The
/// clone-and-rescan reference rejects that capture; production must too.
#[test]
fn production_matches_reference_when_pinned_piece_would_capture_onto_jump() {
    let mut pos = Position {
        board: Board::empty(),
        ..Position::starting()
    };
    pos.board.set(
        Square::from_str("e1").unwrap(),
        Some(Piece {
            color: Color::White,
            kind: PieceKind::King,
        }),
    );
    pos.board.set(
        Square::from_str("e4").unwrap(),
        Some(Piece {
            color: Color::White,
            kind: PieceKind::Rook,
        }),
    );
    pos.board.set(
        Square::from_str("a8").unwrap(),
        Some(Piece {
            color: Color::Black,
            kind: PieceKind::King,
        }),
    );
    pos.board.set(
        Square::from_str("e6").unwrap(),
        Some(Piece {
            color: Color::Black,
            kind: PieceKind::Pawn,
        }),
    );
    pos.board.set(
        Square::from_str("e8").unwrap(),
        Some(Piece {
            color: Color::Black,
            kind: PieceKind::Rook,
        }),
    );
    pos.fields.push(SpellField {
        square: Square::from_str("e6").unwrap(),
        owner: Color::Black,
        kind: SpellKind::Jump,
        expires_after_ply: pos.ply + 1,
    });
    assert_eq!(
        sorted(&legal_moves(&pos)),
        sorted(&legal_moves_reference(&pos))
    );
    let capture = PieceMove::quiet(
        Square::from_str("e4").unwrap(),
        Square::from_str("e6").unwrap(),
    );
    assert!(!legal_moves_reference(&pos).contains(&capture));
}

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
    let mut out = vec![pos];
    for _ in 0..plies {
        let turns = generate_turns(&pos);
        if turns.is_empty() {
            break;
        }
        let pick = (splitmix64(&mut state) as usize) % turns.len();
        pos = apply_turn(&pos, &turns[pick]);
        if pos.board.king_square(Color::White).is_none()
            || pos.board.king_square(Color::Black).is_none()
        {
            break;
        }
        out.push(pos);
    }
    out
}

fn with_field(pos: &Position, kind: SpellKind, square: Square) -> Position {
    let mut next = *pos;
    next.fields.push(SpellField {
        square,
        owner: pos.side_to_move,
        kind,
        expires_after_ply: pos.ply + 1,
    });
    next
}

fn format_pos(pos: &Position) -> String {
    let mut s = String::new();
    for rank in (0..8).rev() {
        for file in 0..8 {
            let sq = Square::new(file, rank);
            match pos.board.get(sq) {
                None => s.push('.'),
                Some(p) => {
                    let c = match p.kind {
                        PieceKind::Pawn => 'p',
                        PieceKind::Knight => 'n',
                        PieceKind::Bishop => 'b',
                        PieceKind::Rook => 'r',
                        PieceKind::Queen => 'q',
                        PieceKind::King => 'k',
                    };
                    s.push(if p.color == Color::White {
                        c.to_ascii_uppercase()
                    } else {
                        c
                    });
                }
            }
        }
        s.push('\n');
    }
    let fields: Vec<_> = pos
        .fields
        .iter()
        .map(|f| format!("{:?}@{} owner={:?}", f.kind, f.square, f.owner))
        .collect();
    s.push_str(&format!(
        "side={:?} ply={} ep={:?} fields={fields:?}",
        pos.side_to_move, pos.ply, pos.en_passant
    ));
    s
}

fn assert_match(pos: &Position, label: &str) {
    let prod = sorted(&legal_moves(pos));
    let refer = sorted(&legal_moves_reference(pos));
    if prod != refer {
        let extra: Vec<_> = prod.iter().filter(|m| !refer.contains(m)).collect();
        let missing: Vec<_> = refer.iter().filter(|m| !prod.contains(m)).collect();
        panic!(
            "{label} at ply {} side {:?}\n{}\nprod extra: {:?}\nprod missing: {:?}",
            pos.ply,
            pos.side_to_move,
            format_pos(pos),
            extra,
            missing
        );
    }
}

fn fuzz_walks_and_spell_hypos(seeds: impl IntoIterator<Item = u64>, plies: u32) {
    let mut positions = vec![(0u64, Position::starting())];
    for seed in seeds {
        positions.extend(
            random_legal_walk(seed, plies)
                .into_iter()
                .map(|p| (seed, p)),
        );
    }
    for (seed, pos) in &positions {
        assert_match(pos, &format!("baseline seed={seed}"));
        if pos.board.king_square(Color::White).is_none()
            || pos.board.king_square(Color::Black).is_none()
        {
            continue;
        }
        for i in 0..64u8 {
            let sq = Square(i);
            let hypo_f = with_field(pos, SpellKind::Freeze, sq);
            assert_match(&hypo_f, &format!("freeze hypo seed={seed} sq={sq}"));
            if pos.board.get(sq).is_some() {
                let hypo_j = with_field(pos, SpellKind::Jump, sq);
                assert_match(&hypo_j, &format!("jump hypo seed={seed} sq={sq}"));
            }
        }
    }
}

#[test]
fn legal_moves_matches_reference_on_random_walks_and_spell_hypos() {
    fuzz_walks_and_spell_hypos(1u64..=16, 8);
}

#[test]
#[ignore]
fn legal_moves_matches_reference_on_dense_random_walks_and_spell_hypos() {
    fuzz_walks_and_spell_hypos(1u64..=64, 12);
}

#[test]
fn jump_through_check_cannot_be_blocked_by_a_rook() {
    // Vector 12 geometry: Bb4 checks Ke1 through jump@d2; Ra1 has no legal moves.
    let mut pos = Position {
        board: Board::empty(),
        ..Position::starting()
    };
    pos.board.set(
        Square::from_str("e1").unwrap(),
        Some(Piece {
            color: Color::White,
            kind: PieceKind::King,
        }),
    );
    pos.board.set(
        Square::from_str("d2").unwrap(),
        Some(Piece {
            color: Color::White,
            kind: PieceKind::Bishop,
        }),
    );
    pos.board.set(
        Square::from_str("a1").unwrap(),
        Some(Piece {
            color: Color::White,
            kind: PieceKind::Rook,
        }),
    );
    pos.board.set(
        Square::from_str("h2").unwrap(),
        Some(Piece {
            color: Color::White,
            kind: PieceKind::Rook,
        }),
    );
    pos.board.set(
        Square::from_str("e8").unwrap(),
        Some(Piece {
            color: Color::Black,
            kind: PieceKind::King,
        }),
    );
    pos.board.set(
        Square::from_str("b4").unwrap(),
        Some(Piece {
            color: Color::Black,
            kind: PieceKind::Bishop,
        }),
    );
    pos.fields.push(SpellField {
        square: Square::from_str("d2").unwrap(),
        owner: Color::Black,
        kind: SpellKind::Jump,
        expires_after_ply: pos.ply + 1,
    });
    assert_eq!(
        sorted(&legal_moves(&pos)),
        sorted(&legal_moves_reference(&pos))
    );
    let a1: Vec<_> = legal_moves(&pos)
        .into_iter()
        .filter(|m| m.from == Square::from_str("a1").unwrap())
        .collect();
    assert!(a1.is_empty());
}

#[test]
fn jumped_pinner_still_pins_the_blocking_pawn() {
    // Ke7 / pc5 / Qb4 with jump@b4: the queen still attacks, so c5-c4 is illegal.
    let mut pos = Position {
        board: Board::empty(),
        ..Position::starting()
    };
    pos.side_to_move = Color::Black;
    pos.board.set(
        Square::from_str("e7").unwrap(),
        Some(Piece {
            color: Color::Black,
            kind: PieceKind::King,
        }),
    );
    pos.board.set(
        Square::from_str("c5").unwrap(),
        Some(Piece {
            color: Color::Black,
            kind: PieceKind::Pawn,
        }),
    );
    pos.board.set(
        Square::from_str("e2").unwrap(),
        Some(Piece {
            color: Color::White,
            kind: PieceKind::King,
        }),
    );
    pos.board.set(
        Square::from_str("b4").unwrap(),
        Some(Piece {
            color: Color::White,
            kind: PieceKind::Queen,
        }),
    );
    pos.fields.push(SpellField {
        square: Square::from_str("b4").unwrap(),
        owner: Color::Black,
        kind: SpellKind::Jump,
        expires_after_ply: pos.ply + 1,
    });
    assert_eq!(
        sorted(&legal_moves(&pos)),
        sorted(&legal_moves_reference(&pos))
    );
    let push = PieceMove::quiet(
        Square::from_str("c5").unwrap(),
        Square::from_str("c4").unwrap(),
    );
    assert!(!legal_moves(&pos).contains(&push));
}

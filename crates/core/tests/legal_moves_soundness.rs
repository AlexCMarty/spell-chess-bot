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

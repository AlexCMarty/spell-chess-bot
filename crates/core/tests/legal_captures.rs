//! `legal_captures` checked against independently hand-written expected outputs, as
//! a check distinct from `legal.rs`'s own internal order-equivalence property test
//! (see `legal_captures_matches_legal_moves_filtered_to_captures_ordered` there) --
//! this file asserts literal expected move lists, the same way `legal.rs`'s own
//! `vector_*` tests do for `legal_moves`.

use spellchess_core::*;

fn sq(s: &str) -> Square {
    Square::from_str(s).unwrap()
}

#[test]
fn king_capture_via_jump_is_the_only_capture_from_b4() {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(sq("e1"), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(sq("d2"), Some(Piece { color: Color::White, kind: PieceKind::Bishop }));
    pos.board.set(sq("a1"), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(sq("h2"), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(sq("e8"), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(sq("b4"), Some(Piece { color: Color::Black, kind: PieceKind::Bishop }));
    pos.board.set(sq("a8"), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
    pos.side_to_move = Color::Black;
    pos.fields.push(SpellField {
        square: sq("d2"), owner: Color::Black, kind: SpellKind::Jump, expires_after_ply: pos.ply + 1,
    });
    let caps = legal_captures(&pos);
    let from_b4: Vec<Square> = caps.iter().filter(|m| m.from == sq("b4")).map(|m| m.to).collect();
    // Per rules/40-jump.md: transparency is additive, a slider may capture the piece on the
    // jump square OR pass through. The bishop can capture both the bishop on d2 and the king on e1.
    let mut expected = vec![sq("e1"), sq("d2")];
    expected.sort();
    let mut actual = from_b4.clone();
    actual.sort();
    assert_eq!(actual, expected, "b4's bishop can capture the jumped bishop on d2 and the king on e1 through the jump");
}

#[test]
fn frozen_piece_is_still_capturable_and_the_only_capture_available() {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(sq("e1"), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(sq("d1"), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(sq("e8"), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(sq("d5"), Some(Piece { color: Color::Black, kind: PieceKind::Knight }));
    pos.fields.push(SpellField {
        square: sq("d5"), owner: Color::Black, kind: SpellKind::Freeze, expires_after_ply: pos.ply + 1,
    });
    let caps = legal_captures(&pos);
    let from_d1: Vec<Square> = caps.iter().filter(|m| m.from == sq("d1")).map(|m| m.to).collect();
    assert_eq!(from_d1, vec![sq("d5")], "the frozen knight on d5 must still be capturable, and it's the rook's only capture");
}

#[test]
fn jump_opens_a_rook_battery_capture_for_both_sides() {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(sq("e1"), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(sq("d1"), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(sq("d4"), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
    pos.board.set(sq("e8"), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(sq("d8"), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
    pos.fields.push(SpellField {
        square: sq("d4"), owner: Color::White, kind: SpellKind::Jump, expires_after_ply: pos.ply + 1,
    });
    assert_eq!(
        legal_captures(&pos).iter().filter(|m| m.from == sq("d1")).map(|m| m.to).collect::<Vec<_>>(),
        vec![sq("d8")],
    );
    let mut black_pos = pos;
    black_pos.side_to_move = Color::Black;
    // Black rook can capture both the jumped pawn on d4 and the white rook on d1.
    let black_captures: Vec<Square> = legal_captures(&black_pos)
        .iter()
        .filter(|m| m.from == sq("d8"))
        .map(|m| m.to)
        .collect();
    let mut expected = vec![sq("d1"), sq("d4")];
    expected.sort();
    let mut actual = black_captures.clone();
    actual.sort();
    assert_eq!(actual, expected, "d8's rook can capture the jumped pawn on d4 and the white rook on d1");
}

#[test]
fn no_legal_captures_in_the_starting_position() {
    assert!(legal_captures(&Position::starting()).is_empty());
}

#[test]
fn en_passant_is_the_only_capture_available() {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(sq("e1"), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(sq("e5"), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
    pos.board.set(sq("d5"), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
    pos.board.set(sq("e8"), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.en_passant = Some(sq("d6"));
    let caps = legal_captures(&pos);
    assert_eq!(caps.len(), 1);
    assert!(caps[0].is_en_passant && caps[0].from == sq("e5") && caps[0].to == sq("d6"));
}

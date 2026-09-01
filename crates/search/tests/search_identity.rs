//! Bit-exact regression net for spell-turn generation refactors.
//!
//! The delta rewrite must not change *which* turns the search sees, only how
//! fast they are produced. Since `search()` is deterministic, that makes the
//! returned turn and score an exact invariant: any drift here is a bug in the
//! new generator, never an acceptable difference.
//!
//! Regenerate after an INTENTIONAL search-behaviour change (and only then):
//!   cargo test -p spellchess-search --release --test search_identity -- \
//!       print_expected --ignored --nocapture

use spellchess_core::{Board, Color, Piece, PieceKind, Position, Square};
use spellchess_search::search::{search, Budget};

fn sparse_rook_endgame() -> Position {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(Square::from_str("h8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
    pos
}

fn freeze_tactic() -> Position {
    // Rd1 can take Nd5, but the c6 pawn recaptures -- freeze the pawn and take.
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(Square::from_str("d1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(Square::from_str("d5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Knight }));
    pos.board.set(Square::from_str("c6").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
    pos
}

fn jump_tactic() -> Position {
    // Black's Bb4 takes Ke1 through a jump on d2.
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(Square::from_str("d2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Bishop }));
    pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(Square::from_str("b4").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Bishop }));
    pos.side_to_move = Color::Black;
    pos
}

fn battery() -> Vec<(&'static str, Position, u32)> {
    vec![
        ("sparse_rook_endgame", sparse_rook_endgame(), 4),
        ("freeze_tactic", freeze_tactic(), 3),
        ("jump_tactic", jump_tactic(), 3),
        ("starting", Position::starting(), 2),
    ]
}

fn actual(pos: &Position, depth: u32) -> (String, i32) {
    let (turn, score) = search(pos, Budget::Depth(depth)).expect("a legal turn exists");
    (spellchess_cli::notation::format_turn(&turn), score)
}

/// (name, depth, formatted turn, score) -- filled in by `print_expected`.
const EXPECTED: &[(&str, u32, &str, i32)] = &[
    ("sparse_rook_endgame", 4, "a1b1", 0),
    ("freeze_tactic", 3, "d1a1", 60),
    ("jump_tactic", 3, "jump@d2 b4e1", 99999),
    ("starting", 2, "b1c3", 0),
];

#[test]
fn search_output_is_unchanged() {
    assert!(!EXPECTED.is_empty(), "EXPECTED is empty -- run the print_expected step first");
    for (name, pos, depth) in battery() {
        let (turn, score) = actual(&pos, depth);
        let want = EXPECTED
            .iter()
            .find(|(n, d, _, _)| *n == name && *d == depth)
            .unwrap_or_else(|| panic!("no EXPECTED row for {name} at depth {depth}"));
        assert_eq!(
            (turn.as_str(), score),
            (want.2, want.3),
            "{name} at depth {depth} drifted: generation changed the search tree",
        );
    }
}

#[test]
#[ignore = "regeneration helper, not a test"]
fn print_expected() {
    for (name, pos, depth) in battery() {
        let (turn, score) = actual(&pos, depth);
        println!("    (\"{name}\", {depth}, \"{turn}\", {score}),");
    }
}

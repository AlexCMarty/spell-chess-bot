// Differential fixtures captured from chess.com's own Spell Chess engine, per
// rules/70-engine-api.md. Each JSON file under tests/fixtures/ pins a position and
// the engine's real output for it; this test re-derives the same shape from
// spellchess_core and asserts they match.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::Deserialize;
use spellchess_core::*;

#[derive(Deserialize)]
struct FixtureSpellCounter {
    count: u8,
    lock: u8,
}

#[derive(Deserialize)]
struct FixtureSpellState {
    freeze: FixtureSpellCounter,
    jump: FixtureSpellCounter,
}

#[derive(Deserialize)]
struct FixtureCastleRights {
    white_kingside: bool,
    white_queenside: bool,
    black_kingside: bool,
    black_queenside: bool,
}

#[derive(Deserialize)]
struct FixtureField {
    square: String,
    owner: String,
    kind: String,
    expires_after_ply: u64,
}

#[derive(Deserialize)]
struct FixtureExpected {
    destinations: BTreeMap<String, Vec<String>>,
    freeze_targets: Vec<String>,
    jump_targets: Vec<String>,
}

#[derive(Deserialize)]
struct Fixture {
    name: String,
    pieces: BTreeMap<String, String>,
    side_to_move: String,
    castle_rights: FixtureCastleRights,
    white_spells: FixtureSpellState,
    black_spells: FixtureSpellState,
    fields: Vec<FixtureField>,
    ply: u64,
    expected: FixtureExpected,
}

fn parse_color(s: &str) -> Color {
    match s {
        "white" => Color::White,
        "black" => Color::Black,
        other => panic!("unknown color: {other}"),
    }
}

fn parse_piece(code: &str) -> Piece {
    let bytes = code.as_bytes();
    assert_eq!(bytes.len(), 2, "malformed piece code: {code}");
    let color = match bytes[0] {
        b'0' => Color::White,
        b'2' => Color::Black,
        other => panic!("unknown piece color digit: {other}"),
    };
    let kind = match bytes[1] {
        b'K' => PieceKind::King,
        b'Q' => PieceKind::Queen,
        b'R' => PieceKind::Rook,
        b'B' => PieceKind::Bishop,
        b'N' => PieceKind::Knight,
        b'P' => PieceKind::Pawn,
        other => panic!("unknown piece kind letter: {other}"),
    };
    Piece { color, kind }
}

fn parse_spell_state(fx: &FixtureSpellState) -> SpellState {
    SpellState {
        freeze: SpellCounter { count: fx.freeze.count, lock: fx.freeze.lock },
        jump: SpellCounter { count: fx.jump.count, lock: fx.jump.lock },
    }
}

fn build_position(fx: &Fixture) -> Position {
    let mut board = Board::empty();
    for (sq, code) in &fx.pieces {
        let square = Square::from_str(sq).unwrap_or_else(|| panic!("bad square: {sq}"));
        board.set(square, Some(parse_piece(code)));
    }

    let mut fields = FieldSet::new();
    for f in &fx.fields {
        fields.push(SpellField {
            square: Square::from_str(&f.square).unwrap_or_else(|| panic!("bad field square: {}", f.square)),
            owner: parse_color(&f.owner),
            kind: match f.kind.as_str() {
                "freeze" => SpellKind::Freeze,
                "jump" => SpellKind::Jump,
                other => panic!("unknown spell kind: {other}"),
            },
            expires_after_ply: f.expires_after_ply,
        });
    }

    Position {
        board,
        side_to_move: parse_color(&fx.side_to_move),
        castle_rights: CastleRights {
            white_kingside: fx.castle_rights.white_kingside,
            white_queenside: fx.castle_rights.white_queenside,
            black_kingside: fx.castle_rights.black_kingside,
            black_queenside: fx.castle_rights.black_queenside,
        },
        en_passant: None,
        halfmove_clock: 0,
        ply: fx.ply,
        white_spells: parse_spell_state(&fx.white_spells),
        black_spells: parse_spell_state(&fx.black_spells),
        fields,
    }
}

fn square_set(squares: &[String]) -> BTreeSet<String> {
    squares.iter().cloned().collect()
}

fn check_fixture(fx: &Fixture) {
    let pos = build_position(fx);
    let mover = pos.side_to_move;

    let mut computed_destinations: BTreeMap<Square, BTreeSet<Square>> = BTreeMap::new();
    for mv in legal_moves(&pos) {
        computed_destinations.entry(mv.from).or_default().insert(mv.to);
    }

    for sq_idx in 0..64u8 {
        let sq = Square(sq_idx);
        let Some(piece) = pos.board.get(sq) else { continue };
        if piece.color != mover {
            continue;
        }
        let computed: BTreeSet<String> = computed_destinations
            .get(&sq)
            .into_iter()
            .flatten()
            .map(|s| s.to_string())
            .collect();
        let expected = fx
            .expected
            .destinations
            .get(&sq.to_string())
            .map(|v| square_set(v))
            .unwrap_or_default();
        assert_eq!(
            computed, expected,
            "fixture {}: destination mismatch for {sq}",
            fx.name
        );
    }

    // Derived from generate_turns' own output, not the raw spells::freeze_targets /
    // jump_targets functions: the real engine's raw target list excludes a square if
    // casting there would leave the mover with zero legal moves at all (there's no way
    // to complete the mandatory move half of the turn), which generate_turns already
    // gets right downstream (it simply emits no Turn for that cast), but the raw
    // exhaustive spells:: functions don't model -- see rules/30-freeze.md and
    // rules/40-jump.md for the sparse-position fixture that surfaced this.
    let turns = generate_turns(&pos);
    let computed_freeze: BTreeSet<String> = turns
        .iter()
        .filter_map(|t| t.spell)
        .filter(|c| c.kind == SpellKind::Freeze)
        .map(|c| c.square.to_string())
        .collect();
    assert_eq!(
        computed_freeze,
        square_set(&fx.expected.freeze_targets),
        "fixture {}: freeze_targets mismatch",
        fx.name
    );

    let computed_jump: BTreeSet<String> = turns
        .iter()
        .filter_map(|t| t.spell)
        .filter(|c| c.kind == SpellKind::Jump)
        .map(|c| c.square.to_string())
        .collect();
    assert_eq!(
        computed_jump,
        square_set(&fx.expected.jump_targets),
        "fixture {}: jump_targets mismatch",
        fx.name
    );
}

#[test]
fn oracle_fixtures_match_engine_output() {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut checked = 0;
    for entry in fs::read_dir(&fixtures_dir).expect("read fixtures dir") {
        let entry = entry.expect("read fixture entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
        let fixture: Fixture = serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {path:?}: {e}"));
        check_fixture(&fixture);
        checked += 1;
    }
    assert!(checked >= 20, "expected at least 20 oracle fixtures, found {checked}");
}

//! Engine types in, JSON out -- and boundary arguments in, engine types out.
//!
//! Nothing here mentions `wasm_bindgen`, so every function in this file is
//! reachable from `cargo test` on the native target. `lib.rs` is the only
//! wasm-aware file in the crate, and it is a shell over this one.

use crate::json;
use spellchess_core::{
    format_turn, game_status, generate_turns, Color, GameStatus, PieceKind, Position, Promotion,
    SpellCast, SpellCounter, SpellKind, Square, Turn,
};

fn color_name(c: Color) -> &'static str {
    match c {
        Color::White => "white",
        Color::Black => "black",
    }
}

fn spell_name(k: SpellKind) -> &'static str {
    match k {
        SpellKind::Freeze => "freeze",
        SpellKind::Jump => "jump",
    }
}

/// `"wP"`, `"bK"` -- colour letter then the standard uppercase piece letter, so
/// the front end can map a square to a glyph with a single lookup.
fn piece_code(kind: PieceKind, color: Color) -> String {
    let k = match kind {
        PieceKind::Pawn => 'P',
        PieceKind::Knight => 'N',
        PieceKind::Bishop => 'B',
        PieceKind::Rook => 'R',
        PieceKind::Queen => 'Q',
        PieceKind::King => 'K',
    };
    let c = if color == Color::White { 'w' } else { 'b' };
    format!("{c}{k}")
}

fn promo_code(p: Promotion) -> &'static str {
    match p {
        Promotion::Queen => "q",
        Promotion::Rook => "r",
        Promotion::Bishop => "b",
        Promotion::Knight => "n",
    }
}

fn counter_json(c: SpellCounter) -> String {
    json::object(&[
        ("count", c.count.to_string()),
        ("lock", c.lock.to_string()),
    ])
}

fn status_json(pos: &Position) -> String {
    match game_status(pos) {
        GameStatus::InProgress => json::object(&[("kind", json::string("inProgress"))]),
        GameStatus::Stalemate => json::object(&[("kind", json::string("stalemate"))]),
        GameStatus::Checkmate(w) => json::object(&[
            ("kind", json::string("checkmate")),
            ("winner", json::string(color_name(w))),
        ]),
        GameStatus::KingCaptured(w) => json::object(&[
            ("kind", json::string("kingCaptured")),
            ("winner", json::string(color_name(w))),
        ]),
    }
}

/// The whole rendered state of a position, in the shape `app.js` draws from.
pub fn state_json(pos: &Position) -> String {
    // Index 0 is a1 and index 63 is h8, matching `Square(rank * 8 + file)`.
    let board: Vec<String> = (0..64u8)
        .map(|i| match pos.board.get(Square(i)) {
            Some(p) => json::string(&piece_code(p.kind, p.color)),
            None => json::string(""),
        })
        .collect();

    let spells = json::object(&[
        (
            "white",
            json::object(&[
                ("freeze", counter_json(pos.white_spells.freeze)),
                ("jump", counter_json(pos.white_spells.jump)),
            ]),
        ),
        (
            "black",
            json::object(&[
                ("freeze", counter_json(pos.black_spells.freeze)),
                ("jump", counter_json(pos.black_spells.jump)),
            ]),
        ),
    ]);

    let fields: Vec<String> = pos
        .fields
        .iter()
        .map(|f| {
            json::object(&[
                ("square", json::string(&f.square.to_string())),
                ("owner", json::string(color_name(f.owner))),
                ("kind", json::string(spell_name(f.kind))),
                ("expiresAfterPly", f.expires_after_ply.to_string()),
            ])
        })
        .collect();

    json::object(&[
        ("board", json::array(&board)),
        ("sideToMove", json::string(color_name(pos.side_to_move))),
        ("ply", pos.ply.to_string()),
        ("spells", spells),
        ("fields", json::array(&fields)),
        ("status", status_json(pos)),
    ])
}

/// One turn as the front end needs it: the squares to highlight, the staged
/// spell, and `text` for the move list.
pub fn turn_json(t: &Turn) -> String {
    let spell = match t.spell {
        Some(cast) => json::object(&[
            ("kind", json::string(spell_name(cast.kind))),
            ("at", json::string(&cast.square.to_string())),
        ]),
        None => "null".to_string(),
    };
    let promo = match t.mv.promotion {
        Some(p) => json::string(promo_code(p)),
        None => "null".to_string(),
    };
    json::object(&[
        ("from", json::string(&t.mv.from.to_string())),
        ("to", json::string(&t.mv.to.to_string())),
        ("promo", promo),
        ("spell", spell),
        ("text", json::string(&format_turn(t))),
    ])
}

pub fn turns_json(turns: &[Turn]) -> String {
    let items: Vec<String> = turns.iter().map(turn_json).collect();
    json::array(&items)
}

pub fn parse_square(s: &str) -> Result<Square, String> {
    Square::from_str(s).ok_or_else(|| format!("not a square: {s:?}"))
}

pub fn parse_promo(s: Option<&str>) -> Result<Option<Promotion>, String> {
    match s {
        None => Ok(None),
        Some("q") => Ok(Some(Promotion::Queen)),
        Some("r") => Ok(Some(Promotion::Rook)),
        Some("b") => Ok(Some(Promotion::Bishop)),
        Some("n") => Ok(Some(Promotion::Knight)),
        Some(other) => Err(format!("not a promotion piece: {other:?}")),
    }
}

/// Half a spell is a front-end bug, not a user action, so it is an error rather
/// than a silent "no spell" -- swallowing it would apply a different turn than
/// the one the user staged.
pub fn parse_spell(kind: Option<&str>, at: Option<&str>) -> Result<Option<SpellCast>, String> {
    match (kind, at) {
        (None, None) => Ok(None),
        (Some(k), Some(a)) => {
            let kind = match k {
                "freeze" => SpellKind::Freeze,
                "jump" => SpellKind::Jump,
                other => return Err(format!("not a spell: {other:?}")),
            };
            Ok(Some(SpellCast { kind, square: parse_square(a)? }))
        }
        _ => Err("a spell needs both a kind and a target square".to_string()),
    }
}

/// Resolves a described turn against the position's legal turns.
///
/// Matching against `generate_turns` -- the exhaustive oracle, not a search
/// generator -- rather than constructing a `Turn` directly is what makes it
/// impossible for the front end to apply something illegal. It also fills in the
/// `is_en_passant` / `is_castle` flags, which the boundary never sends.
pub fn find_turn(
    pos: &Position,
    from: Square,
    to: Square,
    promo: Option<Promotion>,
    spell: Option<SpellCast>,
) -> Result<Turn, String> {
    generate_turns(pos)
        .into_iter()
        .find(|t| t.mv.from == from && t.mv.to == to && t.mv.promotion == promo && t.spell == spell)
        .ok_or_else(|| format!("illegal turn: {from}{to}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellchess_core::{apply_turn, parse_turn, Position};

    #[test]
    fn starting_position_serializes_board_and_spells() {
        let out = state_json(&Position::starting());
        // Index 0 is a1, index 63 is h8 -- Square is rank * 8 + file.
        assert!(out.contains(r#""board":["wR","wN","wB","wQ","wK","wB","wN","wR","wP"#), "got {out}");
        assert!(out.contains(r#""sideToMove":"white""#), "got {out}");
        assert!(out.contains(r#""freeze":{"count":5,"lock":0}"#), "got {out}");
        assert!(out.contains(r#""jump":{"count":2,"lock":0}"#), "got {out}");
        assert!(out.contains(r#""fields":[]"#), "got {out}");
        assert!(out.contains(r#""status":{"kind":"inProgress"}"#), "got {out}");
    }

    /// A cast spell must show up as a live field, because the board tints frozen
    /// squares from exactly this list. If it were dropped, the UI would silently
    /// stop showing that pieces are frozen.
    #[test]
    fn a_cast_spell_appears_in_fields() {
        let pos = Position::starting();
        let turn = parse_turn("freeze@e6 e2e4", &pos).expect("freeze@e6 e2e4 must be legal");
        let next = apply_turn(&pos, &turn);
        let out = state_json(&next);
        assert!(out.contains(r#""square":"e6""#), "got {out}");
        assert!(out.contains(r#""kind":"freeze""#), "got {out}");
        assert!(out.contains(r#""owner":"white""#), "got {out}");
        assert!(out.contains(r#""sideToMove":"black""#), "got {out}");
    }

    #[test]
    fn turns_json_carries_squares_spell_and_display_text() {
        let pos = Position::starting();
        let turns = vec![parse_turn("freeze@e6 e2e4", &pos).unwrap()];
        let out = turns_json(&turns);
        assert_eq!(
            out,
            r#"[{"from":"e2","to":"e4","promo":null,"spell":{"kind":"freeze","at":"e6"},"text":"freeze@e6 e2e4"}]"#
        );
    }

    #[test]
    fn plain_turn_has_null_spell_and_promo() {
        let pos = Position::starting();
        let turns = vec![parse_turn("e2e4", &pos).unwrap()];
        let out = turns_json(&turns);
        assert_eq!(out, r#"[{"from":"e2","to":"e4","promo":null,"spell":null,"text":"e2e4"}]"#);
    }

    #[test]
    fn parse_square_rejects_junk() {
        assert_eq!(parse_square("e4").unwrap(), Square::from_str("e4").unwrap());
        assert!(parse_square("z9").is_err());
        assert!(parse_square("").is_err());
        assert!(parse_square("e44").is_err());
    }

    #[test]
    fn parse_promo_maps_letters_to_variants_and_rejects_junk() {
        assert_eq!(parse_promo(None).unwrap(), None);
        assert_eq!(parse_promo(Some("q")).unwrap(), Some(Promotion::Queen));
        assert_eq!(parse_promo(Some("r")).unwrap(), Some(Promotion::Rook));
        assert_eq!(parse_promo(Some("b")).unwrap(), Some(Promotion::Bishop));
        assert_eq!(parse_promo(Some("n")).unwrap(), Some(Promotion::Knight));
        assert!(parse_promo(Some("k")).is_err());
        assert!(parse_promo(Some("")).is_err());
    }

    /// `turn_json`'s `Some(p)` promo branch, exercised via a directly-constructed
    /// `Turn` rather than a search for a promoting position -- `turn_json` is a
    /// pure function of a `Turn`, so this is a legitimate and much simpler input.
    #[test]
    fn turn_json_renders_a_promotion_letter() {
        use spellchess_core::PieceMove;

        let mv = PieceMove {
            from: Square::from_str("e7").unwrap(),
            to: Square::from_str("e8").unwrap(),
            promotion: Some(Promotion::Queen),
            is_en_passant: false,
            is_castle: false,
        };
        let turn = Turn { spell: None, mv };
        let out = turns_json(&[turn]);
        assert!(out.contains(r#""promo":"q""#), "got {out}");
    }

    #[test]
    fn parse_spell_requires_both_halves() {
        assert!(parse_spell(None, None).unwrap().is_none());
        let cast = parse_spell(Some("freeze"), Some("e6")).unwrap().unwrap();
        assert_eq!(cast.kind, SpellKind::Freeze);
        assert_eq!(cast.square, Square::from_str("e6").unwrap());
        assert!(parse_spell(Some("freeze"), None).is_err(), "half a spell is a protocol bug");
        assert!(parse_spell(Some("fireball"), Some("e6")).is_err());
    }

    /// The UI only ever offers turns from `turns_json`, so reaching this error is a
    /// bug signal rather than a user mistake -- but it must be an error, never a
    /// silently-applied different turn.
    #[test]
    fn find_turn_rejects_an_illegal_turn() {
        let pos = Position::starting();
        let from = Square::from_str("e2").unwrap();
        let to = Square::from_str("e5").unwrap();
        assert!(find_turn(&pos, from, to, None, None).is_err());
    }

    #[test]
    fn find_turn_matches_the_spell_as_well_as_the_move() {
        let pos = Position::starting();
        let from = Square::from_str("e2").unwrap();
        let to = Square::from_str("e4").unwrap();
        let cast = SpellCast { kind: SpellKind::Freeze, square: Square::from_str("e6").unwrap() };
        let found = find_turn(&pos, from, to, None, Some(cast)).unwrap();
        assert_eq!(found.spell, Some(cast), "the staged cast must survive into the applied turn");
        let plain = find_turn(&pos, from, to, None, None).unwrap();
        assert_eq!(plain.spell, None);
    }

    /// Mirrors `terminal::tests::freeze_mate_position` with spells exhausted so the
    /// escape hatch cannot fire -- Black to move, no legal turn, king attacked.
    /// `state_json`/`status_json` must report the checkmate kind and the winner.
    #[test]
    fn status_json_reports_checkmate_and_winner() {
        use spellchess_core::{Board, CastleRights, Piece, SpellCounter, SpellField, SpellState};

        let mut pos = Position { board: Board::empty(), castle_rights: CastleRights::all(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("h8").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.black_spells = SpellState { freeze: SpellCounter { count: 0, lock: 0 }, jump: SpellCounter { count: 0, lock: 0 } };
        pos.fields.push(SpellField {
            square: Square::from_str("e8").unwrap(), owner: Color::White,
            kind: SpellKind::Freeze, expires_after_ply: pos.ply + 1,
        });
        pos.side_to_move = Color::Black;

        assert_eq!(
            game_status(&pos),
            GameStatus::Checkmate(Color::White),
            "fixture must actually be checkmate, or the status_json assertion below is meaningless"
        );
        let out = state_json(&pos);
        assert!(out.contains(r#""status":{"kind":"checkmate","winner":"white"}"#), "got {out}");
    }

    /// Mirrors `terminal::tests::vector_14_stalemate_is_spell_aware`: Black to move,
    /// no legal turn, king not attacked.
    #[test]
    fn status_json_reports_stalemate() {
        use spellchess_core::{Board, CastleRights, Piece, SpellCounter, SpellField, SpellState};

        let mut pos = Position { board: Board::empty(), castle_rights: CastleRights::all(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("b1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.black_spells = SpellState { freeze: SpellCounter { count: 0, lock: 0 }, jump: SpellCounter { count: 0, lock: 0 } };
        pos.fields.push(SpellField {
            square: Square::from_str("e8").unwrap(), owner: Color::White,
            kind: SpellKind::Freeze, expires_after_ply: pos.ply + 1,
        });
        pos.side_to_move = Color::Black;

        assert_eq!(
            game_status(&pos),
            GameStatus::Stalemate,
            "fixture must actually be stalemate, or the status_json assertion below is meaningless"
        );
        let out = state_json(&pos);
        assert!(out.contains(r#""status":{"kind":"stalemate"}"#), "got {out}");
    }

    /// Mirrors `legal::tests::vector_9_king_capture_via_jump`: a live jump field
    /// makes the black bishop's slide to e1 transparent, so it can capture the
    /// white king outright. The resulting position has no white king at all.
    #[test]
    fn status_json_reports_king_captured_and_winner() {
        use spellchess_core::{Board, Piece, PieceMove, SpellField};

        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Bishop }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("h2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("b4").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Bishop }));
        pos.board.set(Square::from_str("a8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
        pos.side_to_move = Color::Black;
        pos.fields.push(SpellField {
            square: Square::from_str("d2").unwrap(), owner: Color::Black,
            kind: SpellKind::Jump, expires_after_ply: pos.ply + 1,
        });

        let capture = PieceMove::quiet(Square::from_str("b4").unwrap(), Square::from_str("e1").unwrap());
        let turn = Turn { spell: None, mv: capture };
        assert!(generate_turns(&pos).contains(&turn), "the capture must be a legal turn, or the fixture is wrong");
        let after = apply_turn(&pos, &turn);

        assert_eq!(game_status(&after), GameStatus::KingCaptured(Color::Black));
        let out = state_json(&after);
        assert!(out.contains(r#""status":{"kind":"kingCaptured","winner":"black"}"#), "got {out}");
    }
}

use spellchess_core::{generate_turns, Position, Promotion, Square, SpellCast, SpellKind, Turn};

pub fn parse_turn(input: &str, pos: &Position) -> Result<Turn, String> {
    let input = input.trim();
    let (spell, rest) = match input.split_once(' ') {
        Some((head, tail)) if head.starts_with("freeze@") || head.starts_with("jump@") => {
            (Some(parse_spell(head)?), tail.trim())
        }
        _ => (None, input),
    };
    let (from, to, promotion) = parse_move_squares(rest)?;
    // Validate the whole turn, not the move in isolation: a spell cast at the
    // start of the turn changes which moves are legal (freeze can immobilise the
    // mover's own piece; jump can unblock a slider), and only `generate_turns`
    // accounts for that.
    generate_turns(pos)
        .into_iter()
        .find(|t| t.spell == spell && t.mv.from == from && t.mv.to == to && t.mv.promotion == promotion)
        .ok_or_else(|| format!("illegal turn: {input}"))
}

fn parse_spell(s: &str) -> Result<SpellCast, String> {
    let (kind_str, sq_str) = s.split_once('@').ok_or_else(|| format!("bad spell: {s}"))?;
    let kind = match kind_str {
        "freeze" => SpellKind::Freeze,
        "jump" => SpellKind::Jump,
        other => return Err(format!("unknown spell: {other}")),
    };
    let square = Square::from_str(sq_str).ok_or_else(|| format!("bad square: {sq_str}"))?;
    Ok(SpellCast { kind, square })
}

fn parse_move_squares(s: &str) -> Result<(Square, Square, Option<Promotion>), String> {
    // The `is_ascii` guard must come before any byte-slicing below: `&s[0..2]` on
    // multi-byte UTF-8 input would panic on a non-char-boundary.
    if !s.is_ascii() || s.len() < 4 {
        return Err(format!("bad move: {s}"));
    }
    let from = Square::from_str(&s[0..2]).ok_or_else(|| format!("bad from-square: {s}"))?;
    let to = Square::from_str(&s[2..4]).ok_or_else(|| format!("bad to-square: {s}"))?;
    let promotion = match s.get(4..5) {
        Some("q") => Some(Promotion::Queen),
        Some("r") => Some(Promotion::Rook),
        Some("b") => Some(Promotion::Bishop),
        Some("n") => Some(Promotion::Knight),
        Some(other) => return Err(format!("bad promotion: {other}")),
        None => None,
    };
    Ok((from, to, promotion))
}

fn spell_name(k: SpellKind) -> &'static str {
    match k {
        SpellKind::Freeze => "freeze",
        SpellKind::Jump => "jump",
    }
}

fn promo_suffix(p: Option<Promotion>) -> &'static str {
    match p {
        Some(Promotion::Queen) => "q",
        Some(Promotion::Rook) => "r",
        Some(Promotion::Bishop) => "b",
        Some(Promotion::Knight) => "n",
        None => "",
    }
}

pub fn format_turn(t: &Turn) -> String {
    let mv = format!("{}{}{}", t.mv.from, t.mv.to, promo_suffix(t.mv.promotion));
    match t.spell {
        Some(cast) => format!("{}@{} {}", spell_name(cast.kind), cast.square, mv),
        None => mv,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellchess_core::{apply_turn, Board, Color, Piece, PieceKind, Position, SpellCounter};

    /// Position after 1.e4, Black to move. Black's d7 pawn blocks its own queen on
    /// d8, which is what makes this position discriminate the two C3 bugs.
    fn after_e4() -> Position {
        let start = Position::starting();
        let e4 = parse_turn("e2e4", &start).unwrap();
        apply_turn(&start, &e4)
    }

    fn round_trip_all(pos: &Position) {
        let turns = generate_turns(pos);
        assert!(!turns.is_empty(), "test position must generate turns");
        for turn in &turns {
            let text = format_turn(turn);
            match parse_turn(&text, pos) {
                Ok(back) => assert_eq!(&back, turn, "round-trip changed the turn: {text}"),
                Err(e) => panic!("engine-generated turn {text} failed to re-parse: {e}"),
            }
        }
    }

    #[test]
    fn parses_a_plain_move() {
        let pos = Position::starting();
        let turn = parse_turn("e2e4", &pos).unwrap();
        assert!(turn.spell.is_none());
        assert_eq!(turn.mv.from, Square::from_str("e2").unwrap());
        assert_eq!(turn.mv.to, Square::from_str("e4").unwrap());
    }

    #[test]
    fn parses_a_spell_and_move() {
        let pos = Position::starting();
        let turn = parse_turn("freeze@d5 e2e4", &pos).unwrap();
        assert_eq!(turn.spell, Some(SpellCast { kind: SpellKind::Freeze, square: Square::from_str("d5").unwrap() }));
    }

    #[test]
    fn rejects_an_illegal_move() {
        let pos = Position::starting();
        assert!(parse_turn("e2e5", &pos).is_err());
    }

    #[test]
    fn format_turn_round_trips_through_parse_turn() {
        let pos = Position::starting();
        let turn = parse_turn("e2e4", &pos).unwrap();
        assert_eq!(format_turn(&turn), "e2e4");
    }

    #[test]
    fn rejects_a_freeze_that_immobilises_its_own_mover() {
        // Black freezes d7 -- its own pawn -- and then tries to move that pawn.
        // Legality must be judged *after* the spell is applied, so this is illegal
        // even though d7d5 is perfectly legal without the freeze.
        let pos = after_e4();
        assert!(parse_turn("d7d5", &pos).is_ok(), "d7d5 alone is legal");
        assert!(
            parse_turn("freeze@d7 d7d5", &pos).is_err(),
            "freezing your own mover then moving it must be rejected"
        );
    }

    #[test]
    fn accepts_a_move_that_only_a_jump_makes_legal() {
        // Qd8-d2 is blocked by Black's own d7 pawn; jump@d7 makes d7 transparent and
        // the queen capture on d2 becomes legal. Validating against the pre-spell
        // position rejected this -- and it is a turn the engine itself suggests.
        let pos = after_e4();
        assert!(parse_turn("d8d2", &pos).is_err(), "d8d2 is blocked without a jump");
        let turn = parse_turn("jump@d7 d8d2", &pos).expect("jump@d7 d8d2 must be legal");
        assert_eq!(format_turn(&turn), "jump@d7 d8d2");
    }

    #[test]
    fn rejects_non_ascii_input_without_panicking() {
        let pos = Position::starting();
        for bad in ["e2é4", "é", "♞e4", "freeze@d5 e2é4"] {
            assert!(parse_turn(bad, &pos).is_err(), "expected {bad:?} to be rejected");
        }
    }

    #[test]
    fn every_generated_turn_round_trips_on_a_freeze_and_jump_position() {
        // Two bare kings: few legal moves, but freeze (all 64 squares) and jump (both
        // occupied squares) are both castable, so this exercises the full spell
        // notation surface exhaustively and cheaply.
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        round_trip_all(&pos);
    }

    #[test]
    fn every_generated_turn_round_trips_on_a_jump_enabled_slider_position() {
        // Rook d1 is blocked by its own d2 pawn; jump@d2 unlocks the whole d-file.
        // Freeze is on cooldown to keep the turn list small (parse_turn is O(turns),
        // so an exhaustive round-trip is O(turns^2)).
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("d2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.white_spells.freeze = SpellCounter { count: 5, lock: 2 };
        let turns = generate_turns(&pos);
        assert!(
            turns.iter().any(|t| t.spell.map(|s| s.square) == Some(Square::from_str("d2").unwrap())
                && t.mv.from == Square::from_str("d1").unwrap()
                && t.mv.to == Square::from_str("d5").unwrap()),
            "the jump-enabled rook move must be present for this test to mean anything",
        );
        round_trip_all(&pos);
    }

    #[test]
    fn starting_position_turns_round_trip_a_representative_sample() {
        // The full board generates ~1800 turns and parse_turn re-generates them all,
        // so an exhaustive sweep is ~35s in a debug build (see the #[ignore]d test
        // below). A fixed stride still covers every region of the list: the no-spell
        // moves first, then each freeze-target group, then each jump-target group.
        let pos = Position::starting();
        let turns = generate_turns(&pos);
        assert!(turns.len() > 1000, "expected a large turn list, got {}", turns.len());
        for turn in turns.iter().step_by(17) {
            let text = format_turn(turn);
            match parse_turn(&text, &pos) {
                Ok(back) => assert_eq!(&back, turn, "round-trip changed the turn: {text}"),
                Err(e) => panic!("engine-generated turn {text} failed to re-parse: {e}"),
            }
        }
    }

    /// Exhaustive version of the sample above (~35s per position in a debug build).
    /// Run with `cargo test -p spellchess-cli -- --ignored`.
    #[test]
    #[ignore = "slow: ~80s in a debug build"]
    fn every_generated_turn_round_trips_on_full_boards() {
        round_trip_all(&Position::starting());
        round_trip_all(&after_e4());
    }
}

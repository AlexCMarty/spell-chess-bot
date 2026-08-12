use spellchess_core::{legal_moves, PieceMove, Position, Promotion, Square, SpellCast, SpellKind, Turn};

pub fn parse_turn(input: &str, pos: &Position) -> Result<Turn, String> {
    let input = input.trim();
    let (spell, rest) = match input.split_once(' ') {
        Some((head, tail)) if head.starts_with("freeze@") || head.starts_with("jump@") => {
            (Some(parse_spell(head)?), tail.trim())
        }
        _ => (None, input),
    };
    let mv = parse_move(rest, pos)?;
    Ok(Turn { spell, mv })
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

fn parse_move(s: &str, pos: &Position) -> Result<PieceMove, String> {
    if s.len() < 4 {
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
    legal_moves(pos)
        .into_iter()
        .find(|m| m.from == from && m.to == to && m.promotion == promotion)
        .ok_or_else(|| format!("illegal move: {s}"))
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
    use spellchess_core::Position;

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
}

use spellchess_core::{Color, PieceKind, Position, Square};

fn glyph(kind: PieceKind, color: Color) -> char {
    let c = match kind {
        PieceKind::Pawn => 'p',
        PieceKind::Knight => 'n',
        PieceKind::Bishop => 'b',
        PieceKind::Rook => 'r',
        PieceKind::Queen => 'q',
        PieceKind::King => 'k',
    };
    if color == Color::White {
        c.to_ascii_uppercase()
    } else {
        c
    }
}

pub fn render_board(pos: &Position) -> String {
    let mut out = String::new();
    for rank in (0..8).rev() {
        out.push_str(&(rank + 1).to_string());
        out.push(' ');
        for file in 0..8 {
            let ch = match pos.board.get(Square::new(file, rank)) {
                Some(p) => glyph(p.kind, p.color),
                None => '.',
            };
            out.push(ch);
            out.push(' ');
        }
        out.push('\n');
    }
    out.push_str("  a b c d e f g h\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellchess_core::{PieceKind, Color};

    #[test]
    fn glyph_uppercases_white_only() {
        assert_eq!(glyph(PieceKind::King, Color::White), 'K');
        assert_eq!(glyph(PieceKind::Pawn, Color::Black), 'p');
    }

    #[test]
    fn render_board_has_nine_lines() {
        let out = render_board(&Position::starting());
        assert_eq!(out.lines().count(), 9);
        assert!(out.lines().next().unwrap().starts_with("8 r n b q k b n r"));
    }
}

use spellchess_core::{Color, PieceKind, Position, Square};

pub fn piece_value(kind: PieceKind) -> i32 {
    match kind {
        PieceKind::Pawn => 100,
        PieceKind::Knight => 320,
        PieceKind::Bishop => 330,
        PieceKind::Rook => 500,
        PieceKind::Queen => 900,
        PieceKind::King => 0,
    }
}

/// Score from the perspective of `pos.side_to_move`: positive is good for the side to move.
pub fn evaluate(pos: &Position) -> i32 {
    let mut white = 0i32;
    let mut black = 0i32;
    for i in 0..64 {
        if let Some(p) = pos.board.get(Square(i)) {
            let v = piece_value(p.kind);
            match p.color {
                Color::White => white += v,
                Color::Black => black += v,
            }
        }
    }
    let material = white - black;
    match pos.side_to_move {
        Color::White => material,
        Color::Black => -material,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellchess_core::Position;

    #[test]
    fn start_position_is_balanced() {
        assert_eq!(evaluate(&Position::starting()), 0);
    }

    #[test]
    fn missing_enemy_queen_favors_the_side_to_move() {
        let mut pos = Position::starting();
        // remove black's queen (d8)
        pos.board.set(Square::from_str("d8").unwrap(), None);
        assert!(evaluate(&pos) > 800);
    }
}

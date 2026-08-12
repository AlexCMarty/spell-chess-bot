use spellchess_core::{Color, PieceKind, Position, Square};

const KNIGHT_PST: [i32; 64] = [
    -20,-10,-10,-10,-10,-10,-10,-20,
    -10,  0,  5,  5,  5,  5,  0,-10,
    -10,  5, 10, 15, 15, 10,  5,-10,
    -10,  5, 15, 20, 20, 15,  5,-10,
    -10,  5, 15, 20, 20, 15,  5,-10,
    -10,  5, 10, 15, 15, 10,  5,-10,
    -10,  0,  5,  5,  5,  5,  0,-10,
    -20,-10,-10,-10,-10,-10,-10,-20,
];

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

fn positional(pos: &Position) -> i32 {
    let mut score = 0i32;
    for i in 0..64u8 {
        if let Some(p) = pos.board.get(Square(i)) {
            if p.kind == PieceKind::Knight {
                let idx = if p.color == Color::White { i as usize } else { 63 - i as usize };
                score += if p.color == Color::White { KNIGHT_PST[idx] } else { -KNIGHT_PST[idx] };
            }
        }
    }
    score
}

fn spell_tempo(pos: &Position, color: Color) -> i32 {
    let s = pos.spells(color);
    s.freeze.count as i32 * 8 + s.jump.count as i32 * 15
        - if s.freeze.lock > 0 { 3 } else { 0 }
        - if s.jump.lock > 0 { 5 } else { 0 }
}

/// Cheap tactical scan: does `color` have a slider that would attack the enemy
/// king if exactly one occupied square between them became transparent, with a
/// jump still available? See rules/50-interactions.md#threat-detection-for-bots.
fn jump_threat_bonus(pos: &Position, color: Color) -> i32 {
    if !pos.spells(color).jump.castable() {
        return 0;
    }
    let enemy_king = match pos.board.king_square(color.opposite()) {
        Some(sq) => sq,
        None => return 0,
    };
    for i in 0..64u8 {
        let sq = Square(i);
        let piece = match pos.board.get(sq) {
            Some(p) if p.color == color && matches!(p.kind, PieceKind::Bishop | PieceKind::Rook | PieceKind::Queen) => p,
            _ => continue,
        };
        let dirs: &[(i8, i8)] = match piece.kind {
            PieceKind::Rook => &spellchess_core::rays::ROOK_DIRS,
            PieceKind::Bishop => &spellchess_core::rays::BISHOP_DIRS,
            PieceKind::Queen => continue, // queen direction covered by rook+bishop cases on other pieces; simple v1 approximation
            _ => continue,
        };
        for &dir in dirs {
            let ray = spellchess_core::rays::walk_ray(pos, sq, dir);
            let blockers: Vec<Square> = ray.iter().copied().filter(|&s| pos.board.get(s).is_some()).collect();
            if blockers.len() == 1 && ray.last() == Some(&enemy_king) {
                return 60;
            }
        }
    }
    0
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
    let position_term = positional(pos);
    let tempo_term = spell_tempo(pos, Color::White) - spell_tempo(pos, Color::Black);
    let threat_term = jump_threat_bonus(pos, Color::White) - jump_threat_bonus(pos, Color::Black);
    let total = material + position_term + tempo_term + threat_term;
    match pos.side_to_move {
        Color::White => total,
        Color::Black => -total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellchess_core::{Board, Piece, Position};

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

    #[test]
    fn centralized_knight_scores_higher_than_rim_knight() {
        let mut centralized = Position { board: Board::empty(), ..Position::starting() };
        centralized.board.set(Square::from_str("d4").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Knight }));
        let mut rim = Position { board: Board::empty(), ..Position::starting() };
        rim.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Knight }));
        assert!(evaluate(&centralized) > evaluate(&rim));
    }

    #[test]
    fn a_live_jump_capture_threat_is_rewarded() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Bishop }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("b4").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Bishop }));
        pos.side_to_move = Color::Black;
        let with_threat = evaluate(&pos);
        pos.black_spells.jump.count = 0;
        let without_threat = evaluate(&pos);
        assert!(with_threat > without_threat);
    }
}

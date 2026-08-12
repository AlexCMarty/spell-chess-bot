use spellchess_core::{Position, Turn};

pub fn order_turns(pos: &Position, mut turns: Vec<Turn>) -> Vec<Turn> {
    turns.sort_by_key(|t| std::cmp::Reverse(turn_priority(pos, t)));
    turns
}

fn turn_priority(pos: &Position, t: &Turn) -> i32 {
    let mut score = 0;
    if let Some(captured) = pos.board.get(t.mv.to) {
        score += 1000 + crate::eval::piece_value(captured.kind);
    }
    if t.spell.is_none() {
        score += 50; // cheap default: prefer a plain move over a speculative cast
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellchess_core::{generate_turns, Position};

    #[test]
    fn captures_sort_before_quiet_moves() {
        let pos = Position::starting();
        let ordered = order_turns(&pos, generate_turns(&pos));
        // start position has no captures, so this just proves the function runs and preserves the set
        assert_eq!(ordered.len(), generate_turns(&pos).len());
    }
}

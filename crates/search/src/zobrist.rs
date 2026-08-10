use spellchess_core::Position;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub fn hash_position(pos: &Position) -> u64 {
    let mut hasher = DefaultHasher::new();
    pos.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellchess_core::Position;

    #[test]
    fn identical_positions_hash_identically() {
        assert_eq!(hash_position(&Position::starting()), hash_position(&Position::starting()));
    }

    #[test]
    fn a_move_changes_the_hash() {
        let start = Position::starting();
        let turns = spellchess_core::generate_turns(&start);
        let after = spellchess_core::apply_turn(&start, &turns[0]);
        assert_ne!(hash_position(&start), hash_position(&after));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    Exact,
    Lower,
    Upper,
}

#[derive(Debug, Clone, Copy)]
pub struct TtEntry {
    pub depth: u32,
    pub score: i32,
    pub bound: Bound,
    pub best_move: Option<spellchess_core::Turn>,
}

const TT_SIZE: usize = 1 << 21;

pub struct TranspositionTable {
    table: Vec<Option<(u64, TtEntry)>>,
}

impl Default for TranspositionTable {
    fn default() -> Self {
        Self::new()
    }
}

impl TranspositionTable {
    pub fn new() -> Self {
        TranspositionTable { table: vec![None; TT_SIZE] }
    }

    fn index(key: u64) -> usize {
        (key as usize) & (TT_SIZE - 1)
    }

    pub fn get(&self, key: u64) -> Option<&TtEntry> {
        match self.table[Self::index(key)] {
            Some((k, ref entry)) if k == key => Some(entry),
            _ => None,
        }
    }

    pub fn insert(&mut self, key: u64, entry: TtEntry) {
        let i = Self::index(key);
        if let Some((old_key, old)) = self.table[i] {
            if old_key != key && old.depth > entry.depth {
                return;
            }
        }
        self.table[i] = Some((key, entry));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_an_entry() {
        use spellchess_core::{PieceMove, Square, Turn};
        let stub_turn = Turn { spell: None, mv: PieceMove::quiet(Square::from_str("e2").unwrap(), Square::from_str("e4").unwrap()) };
        let mut tt = TranspositionTable::new();
        tt.insert(42, TtEntry { depth: 3, score: 17, bound: Bound::Exact, best_move: Some(stub_turn) });
        let entry = tt.get(42).unwrap();
        assert_eq!(entry.depth, 3);
        assert_eq!(entry.score, 17);
        assert_eq!(entry.best_move, Some(stub_turn));
    }
}

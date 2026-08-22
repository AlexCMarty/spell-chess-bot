use spellchess_core::{Square, Turn};

/// Two killer-move slots per ply-from-root bucket. Collisions across branches that
/// happen to share a ply value are the standard, accepted approximation every engine
/// that implements this heuristic makes; it doesn't need to be exact to be useful.
pub struct KillerTable {
    slots: Vec<[Option<Turn>; 2]>,
}

impl KillerTable {
    pub fn new(max_depth: u32) -> KillerTable {
        KillerTable { slots: vec![[None, None]; max_depth as usize + 1] }
    }

    pub fn pair(&self, depth: u32) -> [Option<Turn>; 2] {
        self.slots.get(depth as usize).copied().unwrap_or([None, None])
    }

    /// Records a quiet move that caused a beta cutoff at `ply`. Killers are a
    /// move-ordering hint, not game state -- a depth beyond the table's capacity (or
    /// a move already present) is silently ignored rather than treated as an error;
    /// the worst case is a slightly worse ordering, never incorrect search results.
    pub fn record(&mut self, depth: u32, turn: Turn) {
        let Some(pair) = self.slots.get_mut(depth as usize) else { return };
        if pair[0] == Some(turn) || pair[1] == Some(turn) {
            return;
        }
        pair[1] = pair[0];
        pair[0] = Some(turn);
    }
}

/// From-square/to-square history: how often a quiet move has caused a beta cutoff,
/// weighted by `depth * depth` so cutoffs found deeper in the tree (rarer, more
/// informative) count for more.
pub struct HistoryTable {
    scores: [[u32; 64]; 64],
}

impl Default for HistoryTable {
    fn default() -> Self {
        HistoryTable { scores: [[0; 64]; 64] }
    }
}

impl HistoryTable {
    pub fn new() -> HistoryTable {
        HistoryTable::default()
    }

    pub fn score(&self, from: Square, to: Square) -> u32 {
        self.scores[from.0 as usize][to.0 as usize]
    }

    pub fn record(&mut self, from: Square, to: Square, depth: u32) {
        self.scores[from.0 as usize][to.0 as usize] += depth * depth;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use spellchess_core::PieceMove;

    fn turn(from: &str, to: &str) -> Turn {
        Turn { spell: None, mv: PieceMove::quiet(Square::from_str(from).unwrap(), Square::from_str(to).unwrap()) }
    }

    #[test]
    fn a_fresh_table_has_no_killers_at_any_depth() {
        let table = KillerTable::new(5);
        assert_eq!(table.pair(0), [None, None]);
        assert_eq!(table.pair(5), [None, None]);
    }

    #[test]
    fn recording_a_killer_surfaces_it_in_the_pair_for_that_depth() {
        let mut table = KillerTable::new(5);
        table.record(3, turn("e2", "e4"));
        assert_eq!(table.pair(3), [Some(turn("e2", "e4")), None]);
        assert_eq!(table.pair(2), [None, None], "a killer at depth 3 must not leak into depth 2's slot");
    }

    #[test]
    fn a_second_distinct_killer_fills_the_second_slot_newest_first() {
        let mut table = KillerTable::new(5);
        table.record(3, turn("e2", "e4"));
        table.record(3, turn("d2", "d4"));
        assert_eq!(table.pair(3), [Some(turn("d2", "d4")), Some(turn("e2", "e4"))]);
    }

    #[test]
    fn a_third_distinct_killer_evicts_the_oldest() {
        let mut table = KillerTable::new(5);
        table.record(3, turn("e2", "e4"));
        table.record(3, turn("d2", "d4"));
        table.record(3, turn("c2", "c4"));
        assert_eq!(table.pair(3), [Some(turn("c2", "c4")), Some(turn("d2", "d4"))]);
    }

    #[test]
    fn recording_the_same_killer_twice_does_not_duplicate_it() {
        let mut table = KillerTable::new(5);
        table.record(3, turn("e2", "e4"));
        table.record(3, turn("e2", "e4"));
        assert_eq!(table.pair(3), [Some(turn("e2", "e4")), None]);
    }

    #[test]
    fn recording_past_the_table_s_max_depth_is_a_harmless_no_op() {
        let mut table = KillerTable::new(2);
        table.record(9, turn("e2", "e4")); // beyond max_depth=2 -- must not panic
        assert_eq!(table.pair(9), [None, None]);
    }

    #[test]
    fn a_fresh_history_table_scores_everything_zero() {
        let table = HistoryTable::new();
        assert_eq!(table.score(Square::from_str("e2").unwrap(), Square::from_str("e4").unwrap()), 0);
    }

    #[test]
    fn recording_accumulates_depth_squared() {
        let mut table = HistoryTable::new();
        let (from, to) = (Square::from_str("e2").unwrap(), Square::from_str("e4").unwrap());
        table.record(from, to, 3); // +9
        table.record(from, to, 2); // +4
        assert_eq!(table.score(from, to), 13);
    }

    #[test]
    fn recording_one_move_does_not_affect_another() {
        let mut table = HistoryTable::new();
        table.record(Square::from_str("e2").unwrap(), Square::from_str("e4").unwrap(), 5);
        assert_eq!(table.score(Square::from_str("d2").unwrap(), Square::from_str("d4").unwrap()), 0);
    }
}

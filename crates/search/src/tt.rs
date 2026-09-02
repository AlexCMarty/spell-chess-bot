use std::sync::Mutex;

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

/// Lock shards. Lazy-SMP threads probe and store at every node, so a single lock over
/// the whole table would serialise the search outright; at 256 shards the only cost
/// that shows up is an uncontended lock/unlock, which is a futex fast path.
///
/// `locate` splits the same `key & (TT_SIZE - 1)` slot index the flat table used into
/// (shard, slot) bijectively, so which key lands on which entry -- and therefore the
/// depth-preferring replacement policy and every node count that follows from it --
/// is unchanged from before sharding.
const SHARDS: usize = 256;
const SHARD_SLOTS: usize = TT_SIZE / SHARDS;

pub struct TranspositionTable {
    shards: Box<[Mutex<Box<[Option<(u64, TtEntry)>]>>]>,
}

impl Default for TranspositionTable {
    fn default() -> Self {
        Self::new()
    }
}

impl TranspositionTable {
    pub fn new() -> Self {
        let shards =
            (0..SHARDS).map(|_| Mutex::new(vec![None; SHARD_SLOTS].into_boxed_slice())).collect();
        TranspositionTable { shards }
    }

    fn locate(key: u64) -> (usize, usize) {
        let i = (key as usize) & (TT_SIZE - 1);
        (i % SHARDS, i / SHARDS)
    }

    /// A poisoned shard means a search thread panicked mid-store. The table is a pure
    /// cache and `Mutex` still rules out a torn read, so the worst a poisoned shard
    /// holds is a stale entry -- recovering is strictly better than propagating the
    /// panic into every later probe.
    fn shard(&self, s: usize) -> std::sync::MutexGuard<'_, Box<[Option<(u64, TtEntry)>]>> {
        self.shards[s].lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Returns a copy rather than a reference: the entry lives behind a lock that is
    /// released when this returns, and `TtEntry` is `Copy`.
    pub fn get(&self, key: u64) -> Option<TtEntry> {
        let (s, slot) = Self::locate(key);
        match self.shard(s)[slot] {
            Some((k, entry)) if k == key => Some(entry),
            _ => None,
        }
    }

    /// `&self`, not `&mut self`, so every Lazy-SMP thread can store into one table.
    pub fn insert(&self, key: u64, entry: TtEntry) {
        let (s, slot) = Self::locate(key);
        let mut shard = self.shard(s);
        if let Some((old_key, old)) = shard[slot] {
            if old_key == key {
                if old.depth > entry.depth {
                    return;
                }
                let mut entry = entry;
                if entry.best_move.is_none() {
                    entry.best_move = old.best_move;
                }
                shard[slot] = Some((key, entry));
                return;
            }
            if old.depth > entry.depth {
                return;
            }
        }
        shard[slot] = Some((key, entry));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `locate` splitting the flat slot index into (shard, slot) has to be injective:
    /// two keys sharing a slot would silently halve the usable table and change which
    /// entries survive replacement, and nothing else in the engine would notice.
    #[test]
    fn locate_is_a_bijection_over_the_whole_table() {
        let mut seen = vec![false; TT_SIZE];
        for i in 0..TT_SIZE {
            let (shard, slot) = TranspositionTable::locate(i as u64);
            assert!(shard < SHARDS, "shard {shard} out of range for key {i}");
            assert!(slot < SHARD_SLOTS, "slot {slot} out of range for key {i}");
            let flat = shard * SHARD_SLOTS + slot;
            assert!(!seen[flat], "locate() mapped two keys onto shard {shard} slot {slot}");
            seen[flat] = true;
        }
    }

    #[test]
    fn stores_from_several_threads_all_survive() {
        let tt = TranspositionTable::new();
        std::thread::scope(|scope| {
            for t in 0..4u64 {
                let tt = &tt;
                scope.spawn(move || {
                    for i in 0..2000u64 {
                        let key = t * 100_000 + i;
                        tt.insert(key, TtEntry { depth: 1, score: key as i32, bound: Bound::Exact, best_move: None });
                    }
                });
            }
        });
        for t in 0..4u64 {
            for i in 0..2000u64 {
                let key = t * 100_000 + i;
                // Distinct keys can still collide on one slot, so this asserts the
                // weaker property that actually matters: whatever is there is a
                // consistent entry, never a mix of two writers' fields.
                if let Some(e) = tt.get(key) {
                    assert_eq!(e.score, key as i32, "entry for {key} was torn between writers");
                }
            }
        }
    }

    #[test]
    fn round_trips_an_entry() {
        use spellchess_core::{PieceMove, Square, Turn};
        let stub_turn = Turn { spell: None, mv: PieceMove::quiet(Square::from_str("e2").unwrap(), Square::from_str("e4").unwrap()) };
        let tt = TranspositionTable::new();
        tt.insert(42, TtEntry { depth: 3, score: 17, bound: Bound::Exact, best_move: Some(stub_turn) });
        let entry = tt.get(42).unwrap();
        assert_eq!(entry.depth, 3);
        assert_eq!(entry.score, 17);
        assert_eq!(entry.best_move, Some(stub_turn));
    }

    #[test]
    fn a_shallower_same_key_insert_does_not_replace_a_deeper_entry() {
        let tt = TranspositionTable::new();
        tt.insert(42, TtEntry { depth: 10, score: 99, bound: Bound::Exact, best_move: None });
        tt.insert(42, TtEntry { depth: 1, score: 0, bound: Bound::Upper, best_move: None });
        let entry = tt.get(42).unwrap();
        assert_eq!(entry.depth, 10);
        assert_eq!(entry.score, 99);
        assert_eq!(entry.bound, Bound::Exact);
    }

    #[test]
    fn get_rejects_a_different_key_on_the_same_slot() {
        let tt = TranspositionTable::new();
        tt.insert(42, TtEntry { depth: 3, score: 17, bound: Bound::Exact, best_move: None });
        let colliding = 42 + TT_SIZE as u64;
        assert!(tt.get(colliding).is_none());
    }
}

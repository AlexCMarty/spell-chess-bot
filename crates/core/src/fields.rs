use crate::position::SpellField;

/// Fixed-capacity, `Copy` replacement for `Vec<SpellField>`. Capacity 4 gives a 2x
/// margin over the demonstrated momentary maximum of 2 concurrent fields --
/// steady-state is at most 1.
///
/// `push` always fills the first `None` slot, and `retain` always compacts surviving
/// fields to the front in their original relative order (no gaps left behind). This
/// exactly reproduces `Vec`'s append-after-retain behavior, which matters because
/// `Position` derives `PartialEq`/`Eq`/`Hash` -- `crates/cli/src/repl.rs`'s
/// `assert_eq!(session.pos, before)` relies on two positions with the same logical
/// field contents comparing equal, regardless of the exact push/retain history that
/// produced them. Search hashing is Zobrist (`spellchess_search::zobrist`) and is
/// independent of slot order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FieldSet {
    slots: [Option<SpellField>; 4],
}

impl FieldSet {
    pub fn new() -> FieldSet {
        FieldSet { slots: [None; 4] }
    }

    pub fn push(&mut self, field: SpellField) {
        for slot in self.slots.iter_mut() {
            if slot.is_none() {
                *slot = Some(field);
                return;
            }
        }
        panic!(
            "FieldSet capacity (4) exceeded -- at most 2 fields can be live at once; \
             see the bound argument on this type and rules/20-spell-system.md#how-many-fields-can-be-live-at-once"
        );
    }

    pub fn retain(&mut self, mut f: impl FnMut(&SpellField) -> bool) {
        let mut compacted = [None; 4];
        let mut i = 0;
        for field in self.slots.iter().flatten() {
            if f(field) {
                compacted[i] = Some(*field);
                i += 1;
            }
        }
        self.slots = compacted;
    }

    pub fn iter(&self) -> impl Iterator<Item = &SpellField> {
        self.slots.iter().flatten()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(Option::is_none)
    }

    pub fn len(&self) -> usize {
        self.slots.iter().flatten().count()
    }
}

impl Default for FieldSet {
    fn default() -> FieldSet {
        FieldSet::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::position::SpellKind;
    use crate::types::{Color, Square};

    fn field(square: &str, kind: SpellKind) -> SpellField {
        SpellField { square: Square::from_str(square).unwrap(), owner: Color::White, kind, expires_after_ply: 1 }
    }

    #[test]
    fn new_set_is_empty() {
        let set = FieldSet::new();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
        assert_eq!(set.iter().count(), 0);
    }

    #[test]
    fn push_then_iter_returns_the_pushed_field() {
        let mut set = FieldSet::new();
        set.push(field("d5", SpellKind::Freeze));
        assert_eq!(set.len(), 1);
        assert!(!set.is_empty());
        let collected: Vec<SpellField> = set.iter().copied().collect();
        assert_eq!(collected, vec![field("d5", SpellKind::Freeze)]);
    }

    #[test]
    fn retain_drops_fields_that_fail_the_predicate() {
        let mut set = FieldSet::new();
        set.push(field("d5", SpellKind::Freeze));
        set.push(field("e4", SpellKind::Jump));
        set.retain(|f| f.kind == SpellKind::Jump);
        let collected: Vec<SpellField> = set.iter().copied().collect();
        assert_eq!(collected, vec![field("e4", SpellKind::Jump)]);
    }

    #[test]
    fn retain_compacts_so_a_later_push_reuses_the_freed_slot() {
        // Push 4 (filling capacity), retain down to 1, then push a 5th -- if retain
        // didn't compact, this push would land past the live element and the set
        // would still report capacity exhausted incorrectly on a 6th push.
        let mut set = FieldSet::new();
        for sq in ["a1", "b2", "c3", "d4"] {
            set.push(field(sq, SpellKind::Freeze));
        }
        set.retain(|f| f.square == Square::from_str("d4").unwrap());
        set.push(field("e5", SpellKind::Jump));
        assert_eq!(set.len(), 2);
    }

    #[test]
    #[should_panic(expected = "FieldSet capacity")]
    fn push_past_capacity_panics() {
        let mut set = FieldSet::new();
        for sq in ["a1", "b2", "c3", "d4"] {
            set.push(field(sq, SpellKind::Freeze));
        }
        set.push(field("e5", SpellKind::Jump));
    }

    #[test]
    fn two_sets_built_via_the_same_push_retain_sequence_are_equal() {
        let build = || {
            let mut set = FieldSet::new();
            set.push(field("a1", SpellKind::Freeze));
            set.push(field("b2", SpellKind::Jump));
            set.retain(|f| f.square == Square::from_str("b2").unwrap());
            set.push(field("c3", SpellKind::Freeze));
            set
        };
        assert_eq!(build(), build());
    }
}

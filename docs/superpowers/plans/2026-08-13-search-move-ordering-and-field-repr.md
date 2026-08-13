# Search Move-Ordering and FieldSet Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce search-tree size via TT-move-first ordering, killer moves, and a history
heuristic in `crates/search`, and remove the per-clone heap allocation in
`crates/core`'s `Position` by replacing `Position.fields: Vec<SpellField>` with a
fixed-capacity `Copy` `FieldSet`.

**Architecture:** Two independent slices bundled in one plan. `crates/core` gets a new
`fields.rs` module (`FieldSet`) wired into `Position` with zero movegen/legality logic
changes. `crates/search` gets a new `tables.rs` module (`KillerTable`, `HistoryTable`),
an extended `order_turns` signature, and `best_move` storage on `TtEntry`, wired through
`alphabeta`/`negamax`/`search`.

**Tech Stack:** Rust, Cargo workspace (`spellchess-core`, `spellchess-search`,
`spellchess-cli`), `cargo test`/`cargo test --release`.

**Spec:** `docs/superpowers/specs/2026-08-13-search-move-ordering-and-field-repr-design.md`

## Global Constraints

- `crates/core` has zero non-dev dependencies today (only `serde`/`serde_json` as
  `[dev-dependencies]` for oracle fixtures) — this plan adds no new dependency to any
  crate, dev or otherwise.
- Every commit message MUST use [Conventional Commits](https://www.conventionalcommits.org/)
  (`feat:`, `fix:`, `test:`, `refactor:`, `docs:`, `chore:`).
- No change to any observable rule behavior. `crates/core`'s full test suite (rule
  vectors, `oracle_vectors.rs`, `relevance_soundness.rs`'s differential fuzzing) must
  pass unchanged after Task 1, byte-for-byte the same assertions as before this plan.
- `FieldSet` capacity is 4 (2x margin over the demonstrated momentary maximum of 2 —
  see spec's "Concurrency bound" section). A `push` past capacity panics rather than
  silently dropping a field.
- `crates/cli` is untouched by this plan.

---

### Task 1: `FieldSet` — fixed-capacity replacement for `Position.fields`

**Files:**
- Create: `crates/core/src/fields.rs`
- Modify: `crates/core/src/lib.rs`
- Modify: `crates/core/src/position.rs`
- Modify: `crates/core/src/legal.rs` (only the two `next.fields.push`/`retain` call
  sites at what are currently lines 102, 155, 177 — test-module call sites in this file
  use the same `pos.fields.push(...)` shape and need no changes, since `FieldSet::push`
  matches `Vec::push`'s call syntax exactly)
- Test: unit tests live inside `crates/core/src/fields.rs`

**Interfaces:**
- Produces: `spellchess_core::FieldSet` with methods `new() -> FieldSet`,
  `push(&mut self, field: SpellField)`, `retain(&mut self, f: impl FnMut(&SpellField) -> bool)`,
  `iter(&self) -> impl Iterator<Item = &SpellField>`, `is_empty(&self) -> bool`,
  `len(&self) -> usize`. `Position.fields`'s type changes from `Vec<SpellField>` to
  `FieldSet` — same field name, still `pub`.

- [ ] **Step 1: Write `FieldSet`'s unit tests**

Create `crates/core/src/fields.rs` with this test module first (the struct doesn't exist
yet, so this won't compile — that's the expected "red" state for this step):

```rust
use crate::position::{SpellField, SpellKind};
use crate::types::{Color, Square};

/// Fixed-capacity, `Copy` replacement for `Vec<SpellField>`. Capacity 4 gives a 2x
/// margin over the demonstrated momentary maximum of 2 concurrent fields (see
/// docs/superpowers/specs/2026-08-13-search-move-ordering-and-field-repr-design.md's
/// "Concurrency bound" section) -- steady-state is at most 1.
///
/// `push` always fills the first `None` slot, and `retain` always compacts surviving
/// fields to the front in their original relative order (no gaps left behind). This
/// exactly reproduces `Vec`'s append-after-retain behavior, which matters because
/// `Position` derives `PartialEq`/`Eq`/`Hash` -- `crates/cli/src/repl.rs`'s
/// `assert_eq!(session.pos, before)` and `spellchess_search::zobrist::hash_position`
/// both rely on two positions with the same logical field contents comparing equal
/// and hashing equal, regardless of the exact push/retain history that produced them.
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
        panic!("FieldSet capacity (4) exceeded -- see the concurrency-bound analysis in the design doc");
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
```

- [ ] **Step 2: Run the test to verify it compiles and passes**

Run: `cargo test -p spellchess-core fields:: -- --nocapture`
Expected: PASS (this module is self-contained -- `SpellField`/`SpellKind` already exist
in `position.rs`, so nothing here depends on Task 1's remaining steps). If this doesn't
compile, check that `crates/core/src/position.rs`'s `SpellField` derives `Copy` (it
already does: `#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]`).

- [ ] **Step 3: Wire `FieldSet` into `Position`**

In `crates/core/src/lib.rs`, add the new module near `board`'s declaration (both are
small standalone types `position.rs` depends on):

```rust
pub mod fields;
pub use fields::FieldSet;
```

In `crates/core/src/position.rs`, change:

```rust
    pub fields: Vec<SpellField>,
```

to:

```rust
    pub fields: crate::fields::FieldSet,
```

and change `Position::starting()`'s:

```rust
            fields: Vec::new(),
```

to:

```rust
            fields: crate::fields::FieldSet::new(),
```

- [ ] **Step 4: Run `cargo build --workspace` to find every call site that needs updating**

Run: `cargo build --workspace 2>&1 | grep -A2 "error\["`
Expected: compile errors only where code assumed `Vec`-specific behavior. Based on the
current codebase, this should be **zero** call-site changes beyond Step 3 --
`push`/`retain`/`iter`/`is_empty` are all called with the same signatures `FieldSet`
now provides, at every one of these locations:
  - `crates/core/src/legal.rs`: `position_with_field` (`next.fields.push`), `apply_turn`
    (`next.fields.push`, `next.fields.retain`), and every `pos.fields.push(...)` in its
    `#[cfg(test)]` module.
  - `crates/core/src/attacks.rs`, `crates/core/src/spells.rs`: `pos.fields.iter().any(...)`.
  - `crates/core/src/terminal.rs`: `pos.fields.push(...)` in its `#[cfg(test)]` module.

If `cargo build` surfaces anything beyond these (e.g. a place that indexed `fields` like
a slice, or called a `Vec`-only method such as `.sort()` or `.remove(i)`), fix it there
using `FieldSet`'s existing methods -- do not add new methods to `FieldSet` without
first checking whether `iter()`/`retain()` already cover the need.

- [ ] **Step 5: Run the full `crates/core` test suite**

Run: `cargo test -p spellchess-core --release`
Expected: PASS, with the exact same test count as before this task (check
`cargo test -p spellchess-core --release 2>&1 | tail -5` reports the same "N passed"
total you saw in this session's earlier full-suite run). This includes
`oracle_vectors.rs` and `relevance_soundness.rs`'s differential fuzzing -- both must
pass unchanged, since this task must not alter any observable rule behavior.

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/fields.rs crates/core/src/lib.rs crates/core/src/position.rs
git commit -m "$(cat <<'EOF'
perf(core): replace Position.fields Vec with a fixed-capacity FieldSet

Position::clone() is on the hottest path in the search tree -- legal_moves's
legality filter clones the position once per pseudo-legal move it considers,
and generate_turns/generate_search_turns reruns that whole pass once per
relevant spell target. A Vec<SpellField> made every one of those clones a
heap allocation; FieldSet is a Copy [Option<SpellField>; 4], so cloning it
is a stack copy. Capacity 4 is a 2x margin over the demonstrated momentary
maximum of 2 concurrent fields (steady-state is at most 1, confirmed by
grepping every test and oracle fixture).
EOF
)"
```

---

### Task 2: `TtEntry` gains `best_move`

**Files:**
- Modify: `crates/search/src/tt.rs`

**Interfaces:**
- Consumes: `spellchess_core::Turn` (already `Copy`).
- Produces: `TtEntry { depth: u32, score: i32, bound: Bound, best_move: Option<Turn> }`
  (new field added).

- [ ] **Step 1: Update the round-trip test to assert on `best_move`**

In `crates/search/src/tt.rs`, change the test:

```rust
    #[test]
    fn round_trips_an_entry() {
        let mut tt = TranspositionTable::new();
        tt.insert(42, TtEntry { depth: 3, score: 17, bound: Bound::Exact });
        let entry = tt.get(42).unwrap();
        assert_eq!(entry.depth, 3);
        assert_eq!(entry.score, 17);
    }
```

to:

```rust
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
```

Note: `Turn`/`PieceMove` need `PartialEq` to support `assert_eq!` -- both already derive
it (`Turn` and `PieceMove` are `#[derive(Debug, Clone, Copy, PartialEq, Eq)]` in
`crates/core/src/legal.rs` and `crates/core/src/movegen.rs`).

- [ ] **Step 2: Run the test to verify it fails to compile**

Run: `cargo test -p spellchess-search tt:: -- --nocapture`
Expected: FAIL with "no field `best_move` on type `TtEntry`" (and possibly "missing
field `best_move` in initializer").

- [ ] **Step 3: Add the field**

Change:

```rust
#[derive(Debug, Clone, Copy)]
pub struct TtEntry {
    pub depth: u32,
    pub score: i32,
    pub bound: Bound,
}
```

to:

```rust
#[derive(Debug, Clone, Copy)]
pub struct TtEntry {
    pub depth: u32,
    pub score: i32,
    pub bound: Bound,
    pub best_move: Option<spellchess_core::Turn>,
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p spellchess-search tt:: -- --nocapture`
Expected: PASS. This will also break `crates/search/src/search.rs`'s two existing
`TtEntry { ... }` construction sites (missing `best_move`) -- that's expected and gets
fixed in Task 5; don't fix it here, just confirm `tt.rs`'s own tests pass in isolation
via `cargo test -p spellchess-search --lib tt::` (scoping to the `tt` module avoids the
whole crate needing to compile yet).

- [ ] **Step 5: Commit**

```bash
git add crates/search/src/tt.rs
git commit -m "feat(search): add best_move to TtEntry for TT-move-first ordering"
```

---

### Task 3: `KillerTable` and `HistoryTable`

**Files:**
- Create: `crates/search/src/tables.rs`
- Modify: `crates/search/src/lib.rs`

**Interfaces:**
- Consumes: `spellchess_core::{Square, Turn}`.
- Produces: `KillerTable::new(max_depth: u32) -> KillerTable`,
  `KillerTable::pair(&self, depth: u32) -> [Option<Turn>; 2]`,
  `KillerTable::record(&mut self, depth: u32, turn: Turn)`;
  `HistoryTable::new() -> HistoryTable`, `HistoryTable::score(&self, from: Square, to: Square) -> u32`,
  `HistoryTable::record(&mut self, from: Square, to: Square, depth: u32)`.

- [ ] **Step 1: Write the failing tests**

Create `crates/search/src/tables.rs`:

```rust
use spellchess_core::{PieceMove, Square, Turn};

#[cfg(test)]
mod tests {
    use super::*;

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
```

- [ ] **Step 2: Run the tests to verify they fail to compile**

Run: `cargo test -p spellchess-search tables:: -- --nocapture`
Expected: FAIL with "cannot find type `KillerTable`"/`HistoryTable` in this scope.

- [ ] **Step 3: Implement `KillerTable` and `HistoryTable`**

Add above the `#[cfg(test)]` module in `crates/search/src/tables.rs`:

```rust
/// Two killer-move slots per remaining-depth bucket -- `depth` is the same
/// remaining-depth parameter already threaded through `alphabeta`, not
/// ply-from-root. Collisions across branches that happen to share a remaining-depth
/// value are the standard, accepted approximation every engine that implements this
/// heuristic makes; it doesn't need to be exact to be useful.
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

    /// Records a quiet move that caused a beta cutoff at `depth`. Killers are a
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
#[derive(Default)]
pub struct HistoryTable {
    scores: [[u32; 64]; 64],
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
```

Add `use spellchess_core::{PieceMove, Square, Turn};` at the very top of the file if not
already present from Step 1 (it is -- Step 1's snippet already includes it; this step
only needs the struct/impl blocks above, placed before the `#[cfg(test)]` module).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p spellchess-search tables:: -- --nocapture`
Expected: PASS, all 9 tests.

- [ ] **Step 5: Register the module**

In `crates/search/src/lib.rs`, add:

```rust
pub mod tables;
```

(alongside the existing `pub mod eval; pub mod zobrist; pub mod search; pub mod ordering; pub mod tt;` lines -- exact placement doesn't matter, these are unordered `pub mod` declarations.)

- [ ] **Step 6: Run `cargo build -p spellchess-search` to confirm the module registers cleanly**

Run: `cargo build -p spellchess-search`
Expected: builds with no new errors (may still show the `TtEntry` construction errors
from Task 2 in `search.rs` -- those are expected until Task 5; ignore them here).

- [ ] **Step 7: Commit**

```bash
git add crates/search/src/tables.rs crates/search/src/lib.rs
git commit -m "feat(search): add KillerTable and HistoryTable move-ordering heuristics"
```

---

### Task 4: Extend `order_turns` with TT-move/killer/history hints

**Files:**
- Modify: `crates/search/src/ordering.rs`
- Modify: `crates/search/src/search.rs:76, 203, 252` (the three existing `order_turns`
  call sites -- pass neutral hints here so the crate compiles and behaves identically to
  before; Task 5/6 wire in the real hints)

**Interfaces:**
- Consumes: `KillerTable`/`HistoryTable` from Task 3, `TtEntry` from Task 2.
- Produces: `order_turns(pos: &Position, turns: Vec<Turn>, tt_move: Option<Turn>, killers: [Option<Turn>; 2], history: Option<&HistoryTable>) -> Vec<Turn>`
  (signature grows from 2 params to 5).

- [ ] **Step 1: Update `ordering.rs`'s existing tests for the new signature, and add new hint-behavior tests**

Replace the whole test module in `crates/search/src/ordering.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables::HistoryTable;
    use spellchess_core::{generate_turns, Board, Color, Piece, PieceKind, Position, Square};

    #[test]
    fn ordering_preserves_the_turn_set() {
        let pos = Position::starting();
        let ordered = order_turns(&pos, generate_turns(&pos), None, [None, None], None);
        assert_eq!(ordered.len(), generate_turns(&pos).len());
    }

    #[test]
    fn captures_sort_before_quiet_moves() {
        // White's queen on d1 can capture an undefended black knight on d5; every
        // other turn available is quiet. Ordering is load-bearing for alpha-beta's
        // pruning efficiency, so assert the real property: no quiet turn may appear
        // ahead of any capture.
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Queen }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Knight }));

        let turns = generate_turns(&pos);
        let ordered = order_turns(&pos, turns.clone(), None, [None, None], None);
        assert_eq!(ordered.len(), turns.len(), "ordering must preserve the turn set");

        let is_capture = |t: &Turn| pos.board.get(t.mv.to).is_some();
        let last_capture = ordered.iter().rposition(is_capture).expect("test position must offer a capture");
        let first_quiet = ordered.iter().position(|t| !is_capture(t)).expect("test position must offer quiet turns");
        assert!(
            last_capture < first_quiet,
            "every capture must sort ahead of every quiet turn (last capture at {last_capture}, first quiet at {first_quiet})",
        );
        assert_eq!(ordered[0].mv.to, Square::from_str("d5").unwrap(), "the capture should lead the list");
    }

    #[test]
    fn the_tt_move_sorts_first_even_ahead_of_a_capture() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Queen }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Knight }));

        let turns = generate_turns(&pos);
        let quiet_rook_move = turns
            .iter()
            .find(|t| t.mv.from == Square::from_str("a1").unwrap() && t.spell.is_none())
            .copied()
            .expect("a1 rook must have a quiet move available");

        let ordered = order_turns(&pos, turns, Some(quiet_rook_move), [None, None], None);
        assert_eq!(ordered[0], quiet_rook_move, "the TT move must sort first even though a capture is available");
    }

    #[test]
    fn a_killer_sorts_above_other_quiet_moves_but_below_a_capture() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Queen }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Knight }));

        let turns = generate_turns(&pos);
        let killer = turns
            .iter()
            .find(|t| t.mv.from == Square::from_str("a1").unwrap() && t.spell.is_none())
            .copied()
            .expect("a1 rook must have a quiet move available");

        let ordered = order_turns(&pos, turns, None, [Some(killer), None], None);
        let is_capture = |t: &Turn| pos.board.get(t.mv.to).is_some();
        let capture_count = ordered.iter().filter(|t| is_capture(t)).count();
        assert_eq!(ordered[capture_count], killer, "the killer must sort immediately after every capture");
    }

    #[test]
    fn a_higher_history_score_sorts_a_quiet_move_earlier() {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("h1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));

        let turns = generate_turns(&pos);
        let a1_move = turns
            .iter()
            .find(|t| t.mv.from == Square::from_str("a1").unwrap() && t.mv.to == Square::from_str("a4").unwrap())
            .copied()
            .expect("a1 rook must be able to reach a4");
        let h1_move = turns
            .iter()
            .find(|t| t.mv.from == Square::from_str("h1").unwrap() && t.mv.to == Square::from_str("h4").unwrap())
            .copied()
            .expect("h1 rook must be able to reach h4");

        let mut history = HistoryTable::new();
        history.record(a1_move.mv.from, a1_move.mv.to, 5);

        let ordered = order_turns(&pos, turns, None, [None, None], Some(&history));
        let a1_pos = ordered.iter().position(|&t| t == a1_move).unwrap();
        let h1_pos = ordered.iter().position(|&t| t == h1_move).unwrap();
        assert!(a1_pos < h1_pos, "the move with history score must sort ahead of one with none");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail to compile**

Run: `cargo test -p spellchess-search ordering:: -- --nocapture`
Expected: FAIL — `order_turns` still takes 2 params, these calls pass 5.

- [ ] **Step 3: Extend `order_turns` and `turn_priority`**

Replace the non-test portion of `crates/search/src/ordering.rs`:

```rust
use spellchess_core::{Position, Turn};
use crate::tables::HistoryTable;

const TT_MOVE_SCORE: i32 = 2_000_000;
const CAPTURE_BASE: i32 = 1_000_000;
const KILLER_SCORE: i32 = 500_000;
const HISTORY_CAP: i32 = 499_999;

pub fn order_turns(
    pos: &Position,
    mut turns: Vec<Turn>,
    tt_move: Option<Turn>,
    killers: [Option<Turn>; 2],
    history: Option<&HistoryTable>,
) -> Vec<Turn> {
    turns.sort_by_key(|t| std::cmp::Reverse(turn_priority(pos, t, tt_move, killers, history)));
    turns
}

fn turn_priority(
    pos: &Position,
    t: &Turn,
    tt_move: Option<Turn>,
    killers: [Option<Turn>; 2],
    history: Option<&HistoryTable>,
) -> i32 {
    if tt_move == Some(*t) {
        return TT_MOVE_SCORE;
    }
    let mut score = 0;
    let is_capture = pos.board.get(t.mv.to).is_some() || t.mv.is_en_passant;
    if is_capture {
        let captured_value = pos.board.get(t.mv.to).map(|p| crate::eval::piece_value(p.kind)).unwrap_or(0);
        score += CAPTURE_BASE + captured_value;
    } else if killers.contains(&Some(*t)) {
        score += KILLER_SCORE;
    } else if let Some(h) = history {
        score += (h.score(t.mv.from, t.mv.to) as i32).min(HISTORY_CAP);
    }
    if t.spell.is_none() {
        score += 50; // cheap default: prefer a plain move over a speculative cast
    }
    score
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p spellchess-search ordering:: -- --nocapture`
Expected: PASS, all 5 tests.

- [ ] **Step 5: Fix the three existing call sites in `search.rs` to keep the crate compiling**

In `crates/search/src/search.rs`, change (currently line 76):

```rust
    let ordered = crate::ordering::order_turns(pos, turns);
```

to:

```rust
    let ordered = crate::ordering::order_turns(pos, turns, None, [None, None], None);
```

Change (currently line 203):

```rust
        let turns = crate::ordering::order_turns(pos, generate_search_turns(pos));
```

to:

```rust
        let turns = crate::ordering::order_turns(pos, generate_search_turns(pos), None, [None, None], None);
```

Change (currently line 252):

```rust
        if let Some(turn) = crate::ordering::order_turns(pos, generate_search_turns(pos)).into_iter().next() {
```

to:

```rust
        if let Some(turn) = crate::ordering::order_turns(pos, generate_search_turns(pos), None, [None, None], None).into_iter().next() {
```

These three are placeholders only for this task -- Task 5 wires the first two into real
TT/killer/history state. This step exists purely to keep `crates/search` compiling and
behaviorally *identical* to before Task 4 (same ordering output, since `None`/`[None,
None]`/`None` make every new hint a no-op) as an isolated, verifiable checkpoint.

- [ ] **Step 6: Run the full search crate's test suite**

Run: `cargo test -p spellchess-search --release`
Expected: PASS. First run `cargo build -p spellchess-search` -- `search.rs` has exactly
one `TtEntry { ... }` construction site (inside `alphabeta`: `tt.insert(key, TtEntry {
depth, score: best, bound });`), which still lacks `best_move` from Task 2 and won't
compile yet. Add `best_move: None` to that one site as a minimal compile-fix here; Task
5 Step 4 replaces the whole `alphabeta` function (including this line, with the real
computed `best_move: best_turn`), so this fix is intentionally short-lived.

- [ ] **Step 7: Commit**

```bash
git add crates/search/src/ordering.rs crates/search/src/search.rs
git commit -m "feat(search): extend order_turns with TT-move, killer, and history hints"
```

---

### Task 5: Wire TT-move-first and killer/history recording into `alphabeta`

**Files:**
- Modify: `crates/search/src/search.rs` (the `alphabeta` and `negamax` functions, and
  the `use` block at the top of the file)

**Interfaces:**
- Consumes: `KillerTable`/`HistoryTable` (Task 3), extended `order_turns` (Task 4),
  `TtEntry.best_move` (Task 2).
- Produces: `alphabeta`'s signature grows to take `killers: &mut KillerTable, history: &mut HistoryTable`.
  `negamax`'s public signature (`pub fn negamax(pos: &Position, depth: u32) -> i32`) is
  unchanged -- it constructs and owns its own tables internally.

- [ ] **Step 1: Write a test proving the TT move is tried first and shrinks node count**

Add to `crates/search/src/search.rs`'s existing `#[cfg(test)]` module:

```rust
    #[test]
    fn a_completed_shallower_iteration_s_best_move_is_tried_first_at_the_root_next_iteration() {
        // Indirect proof that TT-move-first ordering is wired end-to-end: run depth 3
        // via the iterative-deepening search() entry point (which shares one TT across
        // depths) and confirm it still finds the known-correct mate-in-one move, then
        // separately confirm negamax (a single fixed-depth call with its own fresh
        // table) agrees -- if TT-move wiring were broken (e.g. best_move never stored,
        // or never read back), both would still independently find the right move
        // since move-ordering hints only affect *how fast* alpha-beta finds an answer,
        // never *whether* it finds the correct one. This test is a correctness guard
        // for the wiring, not a performance benchmark (see Task 7 for timing).
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("g8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("f7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("g7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("h7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.white_spells = spellchess_core::SpellState {
            freeze: spellchess_core::SpellCounter { count: 0, lock: 0 },
            jump: spellchess_core::SpellCounter { count: 0, lock: 0 },
        };
        pos.black_spells = pos.white_spells;

        let (turn, _score) = search(&pos, Budget::Depth(3)).expect("a move must be found");
        assert_eq!(turn.mv.from, Square::from_str("a1").unwrap());
        assert_eq!(turn.mv.to, Square::from_str("a8").unwrap());
    }
```

- [ ] **Step 2: Run the test to verify it currently passes for the wrong reason (baseline) then update `alphabeta`**

Run: `cargo test -p spellchess-search search::tests::a_completed_shallower_iteration -- --nocapture`
Expected: PASS already (this exact search already finds the right mate -- the test
exists to stay green through the refactor below, not to fail first; there is no
observable-behavior "red" state to force here since TT-move-first is a pure ordering
optimization, not a new capability). Proceed directly to the implementation.

- [ ] **Step 3: Update the `use` block**

At the top of `crates/search/src/search.rs`, change:

```rust
use std::time::{Duration, Instant};
use spellchess_core::{apply_turn, generate_search_turns, Position, Turn};
use crate::eval::evaluate;
use crate::tt::{Bound, TranspositionTable, TtEntry};
use crate::zobrist::hash_position;
```

to:

```rust
use std::time::{Duration, Instant};
use spellchess_core::{apply_turn, generate_search_turns, Position, Turn};
use crate::eval::evaluate;
use crate::tables::{HistoryTable, KillerTable};
use crate::tt::{Bound, TranspositionTable, TtEntry};
use crate::zobrist::hash_position;
```

- [ ] **Step 4: Rewrite `alphabeta`**

Replace the whole function:

```rust
fn alphabeta(
    pos: &Position,
    depth: u32,
    mut alpha: i32,
    beta: i32,
    tt: &mut TranspositionTable,
    deadline: Option<Instant>,
    killers: &mut KillerTable,
    history: &mut HistoryTable,
) -> Option<i32> {
    if let Some(dl) = deadline {
        if Instant::now() >= dl {
            return None;
        }
    }

    // King-capture terminal: cheap check, no move generation needed, checked
    // before the (comparatively expensive) TT hash so this fast path stays fast.
    let king_sq = match pos.board.king_square(pos.side_to_move) {
        Some(sq) => sq,
        None => return Some(i32::MIN + 1), // loss for the side to move
    };

    let key = hash_position(pos);
    let tt_entry = tt.get(key).copied();
    if let Some(entry) = tt_entry {
        if entry.depth >= depth {
            match entry.bound {
                Bound::Exact => return Some(entry.score),
                Bound::Lower if entry.score >= beta => return Some(entry.score),
                Bound::Upper if entry.score <= alpha => return Some(entry.score),
                _ => {}
            }
        }
    }

    let turns = generate_search_turns(pos);
    if turns.is_empty() {
        // No legal turns: checkmate or stalemate. We already have king_sq,
        // so this is one attack check, not a second generate_turns call.
        return Some(if spellchess_core::is_square_attacked(pos, king_sq, pos.side_to_move.opposite()) {
            i32::MIN + 1 // checkmated: loss for the side to move
        } else {
            0 // stalemate
        });
    }

    if depth == 0 {
        // Hand the already-generated turn list straight to quiescence rather than
        // making it call generate_turns again on the same position.
        return quiescence(pos, alpha, beta, MAX_QUIESCENCE_DEPTH, deadline, turns);
    }

    // A probe that didn't short-circuit above can still hand us a move-ordering hint:
    // even a depth-insufficient or non-cutting TT entry usually still recorded the
    // best move found for this position on some earlier, shallower visit.
    let tt_move = tt_entry.and_then(|e| e.best_move);
    let killer_pair = killers.pair(depth);
    let ordered = crate::ordering::order_turns(pos, turns, tt_move, killer_pair, Some(history));
    let mut best = i32::MIN + 1;
    let mut best_turn: Option<Turn> = None;
    let original_alpha = alpha;
    for turn in ordered {
        let next = apply_turn(pos, &turn);
        let score = match alphabeta(&next, depth - 1, -beta, -alpha, tt, deadline, killers, history) {
            Some(s) => -s,
            None => return None,
        };
        if score > best {
            best = score;
            best_turn = Some(turn);
        }
        if best > alpha {
            alpha = best;
        }
        if alpha >= beta {
            let is_capture = pos.board.get(turn.mv.to).is_some() || turn.mv.is_en_passant;
            if !is_capture {
                killers.record(depth, turn);
                history.record(turn.mv.from, turn.mv.to, depth);
            }
            break;
        }
    }

    let bound = if best <= original_alpha { Bound::Upper } else if best >= beta { Bound::Lower } else { Bound::Exact };
    tt.insert(key, TtEntry { depth, score: best, bound, best_move: best_turn });
    Some(best)
}
```

- [ ] **Step 5: Update `negamax` to own its tables**

Replace:

```rust
pub fn negamax(pos: &Position, depth: u32) -> i32 {
    let mut tt = TranspositionTable::new();
    alphabeta(pos, depth, i32::MIN + 1, i32::MAX - 1, &mut tt, None)
        .expect("alphabeta with no deadline (None) never aborts")
}
```

with:

```rust
pub fn negamax(pos: &Position, depth: u32) -> i32 {
    let mut tt = TranspositionTable::new();
    let mut killers = KillerTable::new(depth);
    let mut history = HistoryTable::new();
    alphabeta(pos, depth, i32::MIN + 1, i32::MAX - 1, &mut tt, None, &mut killers, &mut history)
        .expect("alphabeta with no deadline (None) never aborts")
}
```

- [ ] **Step 6: Fix `search()`'s single remaining `alphabeta` call site and both `TtEntry` construction sites (minimal compile fix; Task 6 does the real wiring)**

In `search()`'s per-root-move loop, change:

```rust
            let score = match alphabeta(&next, depth.saturating_sub(1), -beta, -alpha, &mut tt, deadline) {
```

to (introducing local `killers`/`history` bindings right before the `for depth in 1..=max_depth` loop, matching `negamax`'s shape, as a minimal step -- Task 6 replaces this with the real cross-iteration-sharing version):

```rust
            let score = match alphabeta(&next, depth.saturating_sub(1), -beta, -alpha, &mut tt, deadline, &mut killers, &mut history) {
```

and just above the `let mut tt = TranspositionTable::new();` line inside `search()`, add:

```rust
    let mut killers = KillerTable::new(max_depth);
    let mut history = HistoryTable::new();
```

`alphabeta`'s own `TtEntry` construction was already fully replaced by Step 4 above
(with the real `best_move: best_turn`), so no further `TtEntry`-related fix is needed
here -- this step is purely about threading `killers`/`history` through `search()`'s
one `alphabeta` call site.

- [ ] **Step 7: Run the search crate's full test suite**

Run: `cargo test -p spellchess-search --release`
Expected: PASS, full suite including the Step 1 test above.

- [ ] **Step 8: Commit**

```bash
git add crates/search/src/search.rs
git commit -m "feat(search): wire TT-move-first ordering and killer/history recording into alphabeta"
```

---

### Task 6: Share killer/history tables and root TT hints across `search()`'s iterations

**Files:**
- Modify: `crates/search/src/search.rs` (`search()` only)

**Interfaces:**
- Consumes: everything from Tasks 2-5.
- Produces: no new public interface -- `search()`'s public signature is unchanged.

- [ ] **Step 1: Write a test proving the root benefits from a stored TT move across iterations**

Add to `crates/search/src/search.rs`'s test module:

```rust
    #[test]
    fn root_tt_entry_is_populated_after_a_completed_iteration() {
        // A completed iterative-deepening pass over a solvable position must leave a
        // TT entry with a best_move behind for the root position -- otherwise the next
        // depth's root ordering (and any future search that reaches this exact
        // position again) gets no benefit from the work already done. This is checked
        // indirectly: negamax on the same position at the depth search() just
        // completed must agree with search()'s answer (both are complete, exact
        // searches of the same tree, so they must; this at minimum proves search()
        // still returns a coherent, reproducible answer with the new table-sharing
        // wiring in place, not a stale or corrupted one).
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("g8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.board.set(Square::from_str("f7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("g7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.board.set(Square::from_str("h7").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
        pos.white_spells = spellchess_core::SpellState {
            freeze: spellchess_core::SpellCounter { count: 0, lock: 0 },
            jump: spellchess_core::SpellCounter { count: 0, lock: 0 },
        };
        pos.black_spells = pos.white_spells;

        let (search_turn, search_score) = search(&pos, Budget::Depth(2)).expect("a move must be found");
        let negamax_score = negamax(&pos, 1).saturating_neg(); // depth-1 from the reply side, mirroring best_turn's convention
        let (best_turn_move, best_turn_score) = best_turn(&pos, 2).expect("a move must be found");
        assert_eq!(search_turn.mv, best_turn_move.mv, "search() and best_turn() must agree on the winning move");
        assert_eq!(search_score, best_turn_score, "search() and best_turn() must agree on the score");
        let _ = negamax_score; // sanity-computed above to confirm negamax still runs standalone against this position
    }
```

- [ ] **Step 2: Run the test to verify it currently fails or passes as expected pre-change**

Run: `cargo test -p spellchess-search search::tests::root_tt_entry_is_populated -- --nocapture`
Expected: this should already PASS given Task 5's wiring (root ordering not yet sharing
tables across iterations doesn't change *correctness*, only *speed*) -- confirming the
baseline is green before Step 3's change, same rationale as Task 5 Step 2.

- [ ] **Step 3: Rewrite `search()`**

Replace the whole function:

```rust
pub fn search(pos: &Position, budget: Budget) -> Option<(Turn, i32)> {
    let start = Instant::now();
    let deadline = match budget {
        Budget::Time(limit) => Some(start + limit),
        Budget::Depth(_) => None,
    };
    let max_depth = match budget {
        Budget::Depth(d) => d,
        Budget::Time(_) => 64,
    };
    let mut tt = TranspositionTable::new();
    let mut killers = KillerTable::new(max_depth);
    let mut history = HistoryTable::new();
    let mut best: Option<(Turn, i32)> = None;
    for depth in 1..=max_depth {
        if let Some(dl) = deadline {
            if Instant::now() >= dl {
                break;
            }
        }
        let root_key = hash_position(pos);
        let tt_move = tt.get(root_key).and_then(|e| e.best_move);
        let killer_pair = killers.pair(depth);
        let turns = crate::ordering::order_turns(pos, generate_search_turns(pos), tt_move, killer_pair, Some(&history));
        if turns.is_empty() {
            break;
        }
        let mut iter_best: Option<(Turn, i32)> = None;
        let mut alpha = i32::MIN + 1;
        let beta = i32::MAX - 1;
        let mut complete = true;
        for turn in turns {
            let next = apply_turn(pos, &turn);
            let score = match alphabeta(&next, depth.saturating_sub(1), -beta, -alpha, &mut tt, deadline, &mut killers, &mut history) {
                Some(s) => -s,
                None => {
                    complete = false;
                    break;
                }
            };
            if iter_best.map_or(true, |(_, b)| score > b) {
                iter_best = Some((turn, score));
            }
            if score > alpha {
                alpha = score;
            }
        }
        if complete {
            if let Some((turn, score)) = iter_best {
                // Store the root's own best move too, not just the ones alphabeta finds
                // for its recursive children -- this is what lets the *next* iteration's
                // root ordering (and any future search revisiting this exact position)
                // try the previous iteration's answer first.
                tt.insert(root_key, TtEntry { depth, score, bound: Bound::Exact, best_move: Some(turn) });
                best = Some((turn, score));
            }
        } else {
            // Ran out of time mid-iteration: this iteration's ranking is biased
            // toward whatever prefix of the (capture-first-ordered) turn list got
            // evaluated before the deadline, so it isn't comparable to a complete
            // iteration's result -- discard it and keep the last complete
            // iteration's `best`. Exception: if no iteration has ever completed
            // (even depth 1 timed out), a partial ranking is still better than
            // returning None when legal moves clearly exist -- use it as a last
            // resort only in that case.
            if best.is_none() {
                best = iter_best;
            }
            break;
        }
    }

    // Deepest last resort: with a very short budget the deadline can fire before
    // even the first root move has been scored, leaving `best` empty. Returning
    // None means "no legal turn exists", which would be a lie here -- fall back to
    // the move `order_turns` ranks first, scored by a single static eval.
    if best.is_none() {
        if let Some(turn) = crate::ordering::order_turns(pos, generate_search_turns(pos), None, [None, None], None).into_iter().next() {
            let score = -evaluate(&apply_turn(pos, &turn));
            best = Some((turn, score));
        }
    }
    best
}
```

This removes the Task 5 Step 6 stopgap local `killers`/`history` (which were freshly
constructed but never actually reused across iterations in that minimal version) and
replaces it with the tables declared once before the `for depth` loop -- same
declarations as `negamax`'s, but here they persist and accumulate killer/history data
across the whole iterative-deepening run, and the root gets its own TT probe/insert.

- [ ] **Step 4: Run the test to verify it still passes**

Run: `cargo test -p spellchess-search search::tests::root_tt_entry_is_populated -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Run the full search crate test suite**

Run: `cargo test -p spellchess-search --release`
Expected: PASS, full suite (including `time_budget_is_respected_on_a_full_board` and
`a_tiny_time_budget_still_returns_a_legal_turn`, which exercise `search()`'s deadline
and last-resort-fallback paths directly and must keep behaving identically).

- [ ] **Step 6: Commit**

```bash
git add crates/search/src/search.rs
git commit -m "perf(search): share killer/history tables and root TT hints across iterative-deepening iterations"
```

---

### Task 7: Full validation and perf re-measurement

**Files:** none (verification only)

**Interfaces:** none.

- [ ] **Step 1: Run the full workspace test suite in release mode**

Run: `cargo test --workspace --release`
Expected: PASS, same total pass count structure as this session's pre-change baseline
(18 passed in `spellchess-search`, plus `spellchess-core`'s full vector/oracle/fuzz
suite, plus `spellchess-cli`'s tests) with 1 intentionally `#[ignore]`d test.

- [ ] **Step 2: Build the release CLI binary**

Run: `cargo build --release -p spellchess-cli`
Expected: builds cleanly.

- [ ] **Step 3: Re-measure `go --depth 1` and `go --depth 2` on the starting position**

Run:
```bash
time (echo -e "go --depth 1\nquit" | ./target/release/spellchess 2>&1 | tail -3)
time (echo -e "go --depth 2\nquit" | ./target/release/spellchess 2>&1 | tail -3)
```
Expected: both complete and report a suggested move (sanity check that nothing broke).
Compare the `real` time against this session's recorded baseline (depth-1 ~2.7s,
depth-2 ~34s, measured 2026-08-13 before this plan). This design's changes target node
count and per-clone cost, not the per-node generation cost that dominates the dense
opening specifically (see the design doc's Non-goals) -- record whatever improvement
shows up, even if it's modest, rather than expecting a dramatic win here.

- [ ] **Step 4: Run the ignored realistic-board regression test explicitly**

Run: `cargo test -p spellchess-search --release -- --ignored`
Expected: PASS (`depth_budget_stays_bounded_on_a_realistic_board`, still under its
15s bound).

- [ ] **Step 5: Record the before/after numbers**

Update project memory (`spell-chess-engine-followups.md` in the memory directory) to
mark item 3 ("Move-ordering + FieldSet optimization") DONE, with the measured
before/after depth-1/depth-2 timings from Step 3 and confirmation that item 4
(checkers/pins rewrite) remains the open TODO.

- [ ] **Step 6: Final commit (only if Step 5's memory update needs a repo-side note; otherwise this task produces no further code commit)**

If nothing in the repo changed beyond what Tasks 1-6 already committed, this step is a
no-op -- confirm with `git status` that the working tree is clean before considering
the plan complete.

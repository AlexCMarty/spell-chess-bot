# Spell-Capture Delta Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the per-leaf "build a hypothetical position, run a full `legal_moves` rescan" approach to spell-enabled capture discovery with direct delta computation, cutting search time roughly 3-4x without changing which turns the search sees.

**Architecture:** A new `crates/core/src/spell_delta.rs` answers "which captures does this spell cast newly make legal?" using bitboard ray arithmetic instead of move generation. It returns `Delta::Complete` when it can settle the question, or `Delta::NeedsRescan` when it declines — in which case the caller runs today's rescan unchanged. Correctness never depends on the fast path being exhaustive, only speed does.

**Tech Stack:** Rust 2021, three-crate Cargo workspace (`spellchess-core` / `spellchess-search` / `spellchess-cli`). No new dependencies — `crates/core` has zero non-dev dependencies and must keep it that way.

**Spec:** `docs/superpowers/specs/2026-09-01-spell-capture-delta-design.md`

## Global Constraints

- **No new dependencies.** `crates/core` has zero non-dev deps. `serde`/`serde_json` are `[dev-dependencies]` only. Do not add a bench framework, a RNG crate, or `rayon`.
- **The turn set must not change.** This pass changes only *how* a turn list is computed, never *which* turns are searched. Any diff in search output is a bug.
- **Never edit an existing rules assertion to make something pass.** `oracle_vectors.rs`, `relevance_soundness.rs` and `legal_moves_soundness.rs` encode `[VERIFIED]` rules. If one goes red, the new code is wrong.
- **Landing requirement:** any change to `generate_quiescence_from` / `generate_quiescence_turns_from` must be re-run against `cargo test -p spellchess-search --release -- --ignored` *before* committing. The item-9 regression sat on `main` for a full commit because this was skipped.
- **Do not swap `jump_targets` for `relevant_jump_targets`** in `generate_quiescence_from`. Tried 2026-08-22, reverted: 13.4s → 18.0s at depth 6, reproduced twice.
- **Cargo lives at `~/.cargo/bin/cargo`** in this environment; plain `cargo` may not resolve on `PATH`.
- Conventional Commits for every commit (`feat:`, `fix:`, `perf:`, `test:`, `docs:`, `refactor:`).

## Baseline (measured 2026-09-01, release, this Pi 5, starting position)

| search | time | nodes | qnodes | nps |
|---|---|---|---|---|
| depth 6 | 13.5s | 340,473 | 2,383,276 | 25,252 |
| depth 8 | 258.5s | 6,453,264 | 17,437,140 | 24,959 |

| function | ns/call |
|---|---|
| `generate_quiescence_turns_from` | 35,981–41,446 |
| `legal_moves` | 911–1,349 |
| `hash_position` | 50 |

---

### Task 1: Measurement and identity infrastructure

No production code changes. This task builds the two instruments every later task depends on: a cost benchmark, and a bit-exact regression net that proves the search output never drifts.

**Files:**
- Create: `crates/search/examples/hotcost.rs`
- Create: `crates/search/tests/search_identity.rs`

**Interfaces:**
- Consumes: nothing (first task).
- Produces: `cargo run --release -p spellchess-search --example hotcost` prints a `ns/call` table. `crates/search/tests/search_identity.rs` holds `EXPECTED: &[(&str, u32, &str, i32)]` — `(position name, depth, formatted turn, score)`.

- [ ] **Step 1: Write the cost benchmark**

Create `crates/search/examples/hotcost.rs`:

```rust
//! Per-call cost of the search hot path. Run before and after any change to
//! spell-turn generation:
//!   cargo run --release -p spellchess-search --example hotcost
//! Debug builds are meaningless here -- always use --release.

use std::hint::black_box;
use std::time::Instant;

use spellchess_core::{
    apply_turn, generate_quiescence_recapture_turns, generate_quiescence_turns_from,
    generate_search_turns, is_square_attacked, legal_moves, pseudo_legal_moves, Position, Turn,
};
use spellchess_search::eval::evaluate;
use spellchess_search::zobrist::hash_position;

fn bench<T>(label: &str, iters: u32, mut f: impl FnMut() -> T) {
    for _ in 0..(iters / 10).max(1) {
        black_box(f());
    }
    let start = Instant::now();
    for _ in 0..iters {
        black_box(f());
    }
    let ns = start.elapsed().as_nanos() as f64 / iters as f64;
    println!("{label:<42} {ns:>10.1} ns/call");
}

fn probe(name: &str, pos: &Position) {
    println!("\n=== {name} ===");
    let baseline = legal_moves(pos);
    let king = pos.board.king_square(pos.side_to_move).unwrap();
    let enemy = pos.side_to_move.opposite();
    println!("(legal moves: {})", baseline.len());

    bench("hash_position", 200_000, || hash_position(pos));
    bench("evaluate", 200_000, || evaluate(pos));
    bench("is_square_attacked(own king)", 200_000, || is_square_attacked(pos, king, enemy));
    bench("pseudo_legal_moves", 50_000, || pseudo_legal_moves(pos));
    bench("legal_moves", 50_000, || legal_moves(pos));
    bench("generate_quiescence_recapture_turns", 20_000, || {
        generate_quiescence_recapture_turns(pos, &baseline)
    });
    bench("generate_quiescence_turns_from", 2_000, || {
        generate_quiescence_turns_from(pos, &baseline)
    });
    bench("generate_search_turns", 500, || generate_search_turns(pos));
    let turn = Turn { spell: None, mv: baseline[0] };
    bench("apply_turn (no spell)", 200_000, || apply_turn(pos, &turn));
}

fn main() {
    let start = Position::starting();
    probe("starting position", &start);

    let mut mid = start;
    for _ in 0..6 {
        let moves = legal_moves(&mid);
        let turn = Turn { spell: None, mv: moves[moves.len() / 2] };
        mid = apply_turn(&mid, &turn);
    }
    probe("6 plies in", &mid);
}
```

- [ ] **Step 2: Run the benchmark and record the numbers**

Run: `~/.cargo/bin/cargo run --release -p spellchess-search --example hotcost`

Expected: a table where `generate_quiescence_turns_from` reads roughly 36,000-41,000 ns/call and `legal_moves` roughly 900-1,350 ns/call. Save this output — Task 6 compares against it.

- [ ] **Step 3: Write the identity test with placeholder expectations**

Create `crates/search/tests/search_identity.rs`. The `EXPECTED` table is filled in by Step 4, not by hand:

```rust
//! Bit-exact regression net for spell-turn generation refactors.
//!
//! The delta rewrite must not change *which* turns the search sees, only how
//! fast they are produced. Since `search()` is deterministic, that makes the
//! returned turn and score an exact invariant: any drift here is a bug in the
//! new generator, never an acceptable difference.
//!
//! Regenerate after an INTENTIONAL search-behaviour change (and only then):
//!   cargo test -p spellchess-search --release --test search_identity -- \
//!       print_expected --ignored --nocapture

use spellchess_core::{Board, Color, Piece, PieceKind, Position, Square};
use spellchess_search::search::{search, Budget};

fn sparse_rook_endgame() -> Position {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(Square::from_str("h8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
    pos
}

fn freeze_tactic() -> Position {
    // Rd1 can take Nd5, but the c6 pawn recaptures -- freeze the pawn and take.
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(Square::from_str("d1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(Square::from_str("d5").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Knight }));
    pos.board.set(Square::from_str("c6").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Pawn }));
    pos
}

fn jump_tactic() -> Position {
    // Black's Bb4 takes Ke1 through a jump on d2.
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(Square::from_str("d2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Bishop }));
    pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(Square::from_str("b4").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Bishop }));
    pos.side_to_move = Color::Black;
    pos
}

fn battery() -> Vec<(&'static str, Position, u32)> {
    vec![
        ("sparse_rook_endgame", sparse_rook_endgame(), 4),
        ("freeze_tactic", freeze_tactic(), 3),
        ("jump_tactic", jump_tactic(), 3),
        ("starting", Position::starting(), 2),
    ]
}

fn actual(pos: &Position, depth: u32) -> (String, i32) {
    let (turn, score) = search(pos, Budget::Depth(depth)).expect("a legal turn exists");
    (spellchess_cli::notation::format_turn(&turn), score)
}

/// (name, depth, formatted turn, score) -- filled in by `print_expected`.
const EXPECTED: &[(&str, u32, &str, i32)] = &[];

#[test]
fn search_output_is_unchanged() {
    assert!(!EXPECTED.is_empty(), "EXPECTED is empty -- run the print_expected step first");
    for (name, pos, depth) in battery() {
        let (turn, score) = actual(&pos, depth);
        let want = EXPECTED
            .iter()
            .find(|(n, d, _, _)| *n == name && *d == depth)
            .unwrap_or_else(|| panic!("no EXPECTED row for {name} at depth {depth}"));
        assert_eq!(
            (turn.as_str(), score),
            (want.2, want.3),
            "{name} at depth {depth} drifted: generation changed the search tree",
        );
    }
}

#[test]
#[ignore = "regeneration helper, not a test"]
fn print_expected() {
    for (name, pos, depth) in battery() {
        let (turn, score) = actual(&pos, depth);
        println!("    (\"{name}\", {depth}, \"{turn}\", {score}),");
    }
}
```

- [ ] **Step 4: Add the dev-dependency the test needs, then generate the expectations**

`search_identity.rs` calls `spellchess_cli::notation::format_turn`. Add to `crates/search/Cargo.toml`:

```toml
[dev-dependencies]
spellchess-cli = { path = "../cli" }
```

Then run: `~/.cargo/bin/cargo test -p spellchess-search --release --test search_identity -- print_expected --ignored --nocapture`

Copy the printed rows verbatim into the `EXPECTED` constant, replacing `&[]`.

`crates/cli/src/lib.rs` already declares `pub mod notation;` and `notation::format_turn(&Turn) -> String` is already public, so no CLI change is needed — only the dev-dependency above.

- [ ] **Step 5: Verify the identity test passes**

Run: `~/.cargo/bin/cargo test -p spellchess-search --release --test search_identity`
Expected: `search_output_is_unchanged` PASSES, `print_expected` reported as ignored.

- [ ] **Step 6: Commit**

```bash
git add crates/search/examples/hotcost.rs crates/search/tests/search_identity.rs crates/search/Cargo.toml
git commit -m "test(search): add hot-path benchmark and bit-exact search identity net"
```

---

### Task 2: `spell_delta` module skeleton, wired but always declining

A pure refactor with provably zero behaviour change: the module exists, the call site consults it, and it always answers `NeedsRescan` so the old path always runs. Task 1's identity test is what proves the wiring is inert.

**Files:**
- Create: `crates/core/src/spell_delta.rs`
- Modify: `crates/core/src/lib.rs` (register and re-export the module)
- Modify: `crates/core/src/legal.rs` (make helpers `pub(crate)`; call the new module from `generate_quiescence_from`)

**Interfaces:**
- Consumes: nothing from Task 1 (that task is test-only).
- Produces:
  - `pub enum Delta { Complete, NeedsRescan }` in `spell_delta`
  - `pub fn captures_enabled_by(pos: &Position, cast: SpellCast, baseline: &[PieceMove], out: &mut Vec<PieceMove>) -> Delta`
  - `legal.rs` items promoted to `pub(crate)`: `PinMap`, `pins_of`, `pin_ray`, `position_with_field`, `king_dest_safe`, `in_baseline`

- [ ] **Step 1: Write the failing test**

Create `crates/core/src/spell_delta.rs` with only this test module plus the API it needs (the API comes in Step 3 — this step is expected not to compile yet):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::legal::legal_moves;
    use crate::position::SpellKind;
    use crate::types::Square;

    #[test]
    fn declining_leaves_the_output_buffer_untouched() {
        let pos = Position::starting();
        let baseline = legal_moves(&pos);
        let cast = SpellCast { kind: SpellKind::Freeze, square: Square::from_str("d4").unwrap() };
        let mut out = vec![baseline[0]];
        let before = out.clone();
        if captures_enabled_by(&pos, cast, &baseline, &mut out) == Delta::NeedsRescan {
            assert_eq!(out, before, "a declining call must not touch `out`");
        }
    }
}
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `~/.cargo/bin/cargo test -p spellchess-core spell_delta`
Expected: FAIL — compile error, `spell_delta` is not declared in `lib.rs` / `captures_enabled_by` not found.

- [ ] **Step 3: Write the minimal module**

Prepend to `crates/core/src/spell_delta.rs`:

```rust
//! Direct computation of the captures a spell cast newly makes legal, replacing
//! the "build a hypothetical position and run a full `legal_moves` rescan" approach
//! that dominated search time (36-41us per leaf node).
//!
//! See docs/superpowers/specs/2026-09-01-spell-capture-delta-design.md.

use crate::legal::SpellCast;
use crate::movegen::PieceMove;
use crate::position::Position;

/// Whether the fast path could settle the question for a given cast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delta {
    /// `out` gained exactly the newly-legal captures. Authoritative.
    Complete,
    /// The fast path declined. The caller must run the rescan; `out` is untouched.
    NeedsRescan,
}

/// Captures that `cast` newly makes legal for `pos.side_to_move`, excluding
/// anything already legal in `baseline`. Appends to `out`.
///
/// Returning `NeedsRescan` is always safe: correctness never depends on this
/// function being exhaustive, only speed does.
pub fn captures_enabled_by(
    pos: &Position,
    cast: SpellCast,
    baseline: &[PieceMove],
    out: &mut Vec<PieceMove>,
) -> Delta {
    let _ = (pos, cast, baseline, out);
    Delta::NeedsRescan
}
```

Add to `crates/core/src/lib.rs`, after the `pub mod spells;` line:

```rust
pub mod spell_delta;
pub use spell_delta::{captures_enabled_by, Delta};
```

Note the import paths: `PieceMove` lives in `crate::movegen`, not `crate::legal` (`legal` owns `Turn` and `SpellCast`). `Bitboard` is not imported yet — it is first needed in Task 3.

- [ ] **Step 4: Run the test to confirm it passes**

Run: `~/.cargo/bin/cargo test -p spellchess-core spell_delta`
Expected: PASS.

- [ ] **Step 5: Promote the helpers `spell_delta` will need**

In `crates/core/src/legal.rs`, change these six declarations from private to `pub(crate)` (signatures otherwise untouched):

```rust
pub(crate) struct PinMap {
    pub(crate) pinned: Bitboard,
    pub(crate) rays: [Bitboard; 64],
}

pub(crate) fn pins_of(pos: &Position, king: Square, us: Color, frozen: Bitboard, jump: Bitboard) -> PinMap

pub(crate) fn pin_ray(pins: &PinMap, from: Square) -> Option<Bitboard>

pub(crate) fn position_with_field(pos: &Position, cast: SpellCast) -> Position

pub(crate) fn king_dest_safe(pos: &Position, king_from: Square, dest: Square, enemy: Color) -> bool

pub(crate) fn in_baseline(baseline: &[PieceMove], mv: PieceMove) -> bool
```

- [ ] **Step 6: Wire the call site in `generate_quiescence_from`**

In `crates/core/src/legal.rs`, replace the jump loop body (currently `legal.rs:583-595`) with the consult-then-fall-back shape. The `else` branch is byte-for-byte the old code:

```rust
    if pos.spells(us).jump.castable() {
        for sq in crate::spells::jump_targets(pos, us) {
            if !jump_may_change_this_ply(pos, sq, us, king_sq, frozen, jump, slider_occ, checkers) {
                continue;
            }
            let cast = SpellCast { kind: SpellKind::Jump, square: sq };
            let mut fast = Vec::new();
            match crate::spell_delta::captures_enabled_by(pos, cast, baseline, &mut fast) {
                crate::spell_delta::Delta::Complete => {
                    for mv in fast {
                        turns.push(Turn { spell: Some(cast), mv });
                    }
                }
                crate::spell_delta::Delta::NeedsRescan => {
                    for mv in legal_moves(&position_with_field(pos, cast)) {
                        if is_capture(pos, &mv) && !in_baseline(baseline, mv) {
                            turns.push(Turn { spell: Some(cast), mv });
                        }
                    }
                }
            }
        }
    }
```

Apply the identical shape to the freeze loop (currently `legal.rs:599-613`), keeping its `freeze_enemy_affects_this_ply` guard exactly as it is:

```rust
    if pos.spells(us).freeze.castable() {
        let their_bb = pos.board.color_bb(enemy);
        for sq in crate::spells::relevant_freeze_targets(pos, us, baseline) {
            let zone = crate::spells::FREEZE_ZONE[sq.0 as usize];
            let hits_them = zone.minus(frozen).intersect(their_bb);
            let cast = SpellCast { kind: SpellKind::Freeze, square: sq };
            if freeze_enemy_affects_this_ply(pos, hits_them, us, king_sq, checkers, &pins, frozen, slider_occ) {
                let mut fast = Vec::new();
                match crate::spell_delta::captures_enabled_by(pos, cast, baseline, &mut fast) {
                    crate::spell_delta::Delta::Complete => {
                        for mv in fast {
                            turns.push(Turn { spell: Some(cast), mv });
                        }
                    }
                    crate::spell_delta::Delta::NeedsRescan => {
                        for mv in legal_moves(&position_with_field(pos, cast)) {
                            if is_capture(pos, &mv) && !in_baseline(baseline, mv) {
                                turns.push(Turn { spell: Some(cast), mv });
                            }
                        }
                    }
                }
            }
        }
    }
```

- [ ] **Step 7: Prove the wiring is inert**

Run all three, all must pass with no assertion edits:

```bash
~/.cargo/bin/cargo test --workspace
~/.cargo/bin/cargo test -p spellchess-search --release --test search_identity
~/.cargo/bin/cargo test -p spellchess-search --release -- --ignored
```

Expected: PASS, PASS, PASS. The identity test passing is the proof that this refactor changed nothing.

- [ ] **Step 8: Commit**

```bash
git add crates/core/src/spell_delta.rs crates/core/src/lib.rs crates/core/src/legal.rs
git commit -m "refactor(core): add spell_delta seam to quiescence spell generation

Always returns NeedsRescan, so the rescan path still runs everywhere and
behaviour is unchanged; the search identity test proves it."
```

---

### Task 3: Differential fuzz harness and the jump delta

The fuzz harness comes first so the jump implementation is written against a live oracle. Jump is the larger half of the cost: `generate_quiescence_from` iterates the exhaustive 32-square `jump_targets`.

**Files:**
- Create: `crates/core/tests/spell_delta_soundness.rs`
- Modify: `crates/core/src/spell_delta.rs`

**Interfaces:**
- Consumes: `captures_enabled_by`, `Delta` (Task 2); `pin_ray`, `pins_of`, `in_baseline` (Task 2, `pub(crate)`).
- Produces: jump casts now return `Delta::Complete` in the common case. No new public API.

- [ ] **Step 1: Write the failing differential test**

Create `crates/core/tests/spell_delta_soundness.rs`:

```rust
//! Differential test: whenever `captures_enabled_by` claims `Complete`, its answer
//! must equal what the rescan it replaces would have produced. `NeedsRescan` falls
//! through to that same rescan at every call site, so only `Complete` needs checking.

use spellchess_core::*;

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

fn random_legal_walk(seed: u64, plies: u32) -> Vec<Position> {
    let mut state = seed;
    let mut pos = Position::starting();
    let mut out = vec![pos];
    for _ in 0..plies {
        let turns = generate_turns(&pos);
        if turns.is_empty() {
            break;
        }
        let pick = (splitmix64(&mut state) as usize) % turns.len();
        pos = apply_turn(&pos, &turns[pick]);
        if pos.board.king_square(Color::White).is_none() || pos.board.king_square(Color::Black).is_none() {
            break;
        }
        out.push(pos);
    }
    out
}

fn sparse_endgame() -> Position {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(Square::from_str("h8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
    pos
}

fn battery() -> Vec<Position> {
    let mut out = vec![Position::starting(), sparse_endgame()];
    for seed in [1u64, 2, 7, 13, 42, 99, 1234, 31337] {
        out.extend(random_legal_walk(seed, 14));
    }
    out
}

fn with_field(pos: &Position, cast: SpellCast) -> Position {
    let mut next = *pos;
    next.fields.push(SpellField {
        square: cast.square,
        owner: pos.side_to_move,
        kind: cast.kind,
        expires_after_ply: pos.ply + 1,
    });
    next
}

/// Exactly what `generate_quiescence_from`'s rescan branch computes.
fn rescan_oracle(pos: &Position, cast: SpellCast, baseline: &[PieceMove]) -> Vec<PieceMove> {
    legal_moves(&with_field(pos, cast))
        .into_iter()
        .filter(|mv| {
            let is_cap = pos.board.get(mv.to).is_some() || mv.is_en_passant;
            is_cap && !baseline.contains(mv)
        })
        .collect()
}

fn sorted(moves: &[PieceMove]) -> Vec<(u8, u8, u8, bool, bool)> {
    let mut v: Vec<_> = moves
        .iter()
        .map(|mv| (mv.from.0, mv.to.0, mv.promotion.map(|p| p as u8).unwrap_or(255), mv.is_en_passant, mv.is_castle))
        .collect();
    v.sort();
    v
}

fn check_kind(kind: SpellKind) -> (u32, u32) {
    let (mut complete, mut declined) = (0u32, 0u32);
    for pos in battery() {
        let us = pos.side_to_move;
        if !match kind {
            SpellKind::Freeze => pos.spells(us).freeze.castable(),
            SpellKind::Jump => pos.spells(us).jump.castable(),
        } {
            continue;
        }
        let baseline = legal_moves(&pos);
        let targets = match kind {
            SpellKind::Freeze => spells::freeze_targets(&pos, us),
            SpellKind::Jump => spells::jump_targets(&pos, us),
        };
        for square in targets {
            let cast = SpellCast { kind, square };
            let mut fast = Vec::new();
            match captures_enabled_by(&pos, cast, &baseline, &mut fast) {
                Delta::NeedsRescan => declined += 1,
                Delta::Complete => {
                    complete += 1;
                    assert_eq!(
                        sorted(&fast),
                        sorted(&rescan_oracle(&pos, cast, &baseline)),
                        "delta disagreed with rescan for {cast:?}",
                    );
                }
            }
        }
    }
    (complete, declined)
}

#[test]
fn jump_delta_matches_the_rescan_oracle() {
    let (complete, declined) = check_kind(SpellKind::Jump);
    println!("jump: {complete} complete, {declined} declined");
    assert!(complete > 0, "jump delta never returned Complete -- it is not wired up");
}

#[test]
fn freeze_delta_matches_the_rescan_oracle() {
    let (complete, declined) = check_kind(SpellKind::Freeze);
    println!("freeze: {complete} complete, {declined} declined");
}
```

- [ ] **Step 2: Run it to confirm the jump test fails**

Run: `~/.cargo/bin/cargo test -p spellchess-core --test spell_delta_soundness`
Expected: `jump_delta_matches_the_rescan_oracle` FAILS with "jump delta never returned Complete -- it is not wired up". `freeze_delta_matches_the_rescan_oracle` passes (it asserts nothing about the count).

- [ ] **Step 3: Implement the jump delta**

In `crates/core/src/spell_delta.rs`, add the imports and replace the body of `captures_enabled_by`:

```rust
use crate::attacks::attackers_to;
use crate::bitboard::Bitboard;
use crate::board::Board;
use crate::legal::{in_baseline, pin_ray, pins_of, SpellCast};
use crate::movegen::PieceMove;
use crate::position::{Position, SpellKind};
use crate::types::{Color, PieceKind, Square};

pub fn captures_enabled_by(
    pos: &Position,
    cast: SpellCast,
    baseline: &[PieceMove],
    out: &mut Vec<PieceMove>,
) -> Delta {
    match cast.kind {
        SpellKind::Jump => jump_captures(pos, cast.square, baseline, out),
        SpellKind::Freeze => Delta::NeedsRescan,
    }
}

/// Squares `kind` attacks from `from` under `occ`. `occ` must already have live
/// jump squares subtracted, so rays run through them.
fn slider_attacks(kind: PieceKind, from: Square, occ: Bitboard) -> Bitboard {
    match kind {
        PieceKind::Bishop => crate::rays::bishop_attacks(from, occ),
        PieceKind::Rook => crate::rays::rook_attacks(from, occ),
        PieceKind::Queen => crate::rays::bishop_attacks(from, occ)
            .union(crate::rays::rook_attacks(from, occ)),
        _ => Bitboard::EMPTY,
    }
}

fn our_sliders(board: &Board, us: Color, frozen: Bitboard) -> Bitboard {
    let sliders = board
        .kind_bb(PieceKind::Bishop)
        .union(board.kind_bb(PieceKind::Rook))
        .union(board.kind_bb(PieceKind::Queen));
    board.color_bb(us).intersect(sliders).minus(frozen)
}

/// Jump only makes a square transparent to sliders (rules/40-jump.md: knights and
/// kings never benefit, and a pawn double-step is not a capture). So the captures a
/// jump newly enables for us are exactly the enemy-occupied squares our sliders
/// attack once the jumped square stops blocking.
fn jump_captures(
    pos: &Position,
    s: Square,
    baseline: &[PieceMove],
    out: &mut Vec<PieceMove>,
) -> Delta {
    let us = pos.side_to_move;
    let enemy = us.opposite();
    let Some(king_sq) = pos.board.king_square(us) else {
        return Delta::NeedsRescan;
    };

    let frozen = crate::spells::frozen_bb(pos);
    let jump_before = crate::spells::jump_bb(pos);
    let occ = pos.board.occupancy();
    let slider_occ_before = occ.minus(jump_before);
    let jump_after = jump_before.with(s);
    let slider_occ_after = occ.minus(jump_after);

    // Jump is symmetric: it can open an enemy slider onto our own king. Recompute
    // our king's check/pin context under the field rather than assuming it holds.
    let checkers_after = attackers_to(pos, king_sq, enemy, frozen, slider_occ_after);
    if checkers_after.count() > 1 {
        // Double check: only king moves are legal, and a king move is never one of
        // the slider captures below. Decline rather than reason about it.
        return Delta::NeedsRescan;
    }
    let single_checker = checkers_after.iter().next();
    let pins_after = pins_of(pos, king_sq, us, frozen, jump_after);
    let enemy_bb = pos.board.color_bb(enemy);

    for p in our_sliders(&pos.board, us, frozen).iter() {
        let kind = pos.board.get(p).expect("slider bitboard square is occupied").kind;
        let gained = slider_attacks(kind, p, slider_occ_after)
            .minus(slider_attacks(kind, p, slider_occ_before))
            .intersect(enemy_bb);
        for t in gained.iter() {
            // King capture legality is the subtlest rule in the engine (the
            // attacker_count comparison in legal_moves); do not duplicate it here.
            if pos.board.get(t).is_some_and(|q| q.kind == PieceKind::King) {
                return Delta::NeedsRescan;
            }
            if let Some(ray) = pin_ray(&pins_after, p) {
                if !ray.contains(t) {
                    continue;
                }
            }
            // In check, a slider capture only helps if it takes the checker;
            // interposing is not a capture.
            if let Some(checker) = single_checker {
                if t != checker {
                    continue;
                }
            }
            let mv = PieceMove::quiet(p, t);
            if in_baseline(baseline, mv) {
                continue;
            }
            out.push(mv);
        }
    }
    Delta::Complete
}
```

- [ ] **Step 4: Run the differential test until it passes**

Run: `~/.cargo/bin/cargo test -p spellchess-core --test spell_delta_soundness -- --nocapture`
Expected: both tests PASS, and the printed line shows a non-zero `complete` count for jump.

If a mismatch is reported, the oracle is right and the delta is wrong. Read the printed `SpellCast` and reproduce that single position before changing anything. Do **not** weaken the assertion.

- [ ] **Step 5: Run the full verification set**

```bash
~/.cargo/bin/cargo test --workspace
~/.cargo/bin/cargo test -p spellchess-search --release --test search_identity
~/.cargo/bin/cargo test -p spellchess-search --release -- --ignored
```

Expected: all PASS. `search_identity` passing means the jump delta produces the same turns the rescan did.

- [ ] **Step 6: Measure the win so far**

```bash
~/.cargo/bin/cargo run --release -p spellchess-search --example hotcost
~/.cargo/bin/cargo build --release -p spellchess-cli
SPELLCHESS_PROFILE=1 ./target/release/spellchess <<'EOF'
go --depth 6
quit
EOF
```

Record `generate_quiescence_turns_from` ns/call and the depth-6 wall time. **Decision point:** the spec targets depth 6 ≤4s. If this already meets it, Tasks 4 and 5 are optional — report the numbers and ask before continuing, rather than doing speculative work on the freeze half.

- [ ] **Step 7: Commit**

```bash
git add crates/core/src/spell_delta.rs crates/core/tests/spell_delta_soundness.rs
git commit -m "perf(core): compute jump-enabled captures by ray delta, not rescan"
```

---

### Task 4: Freeze delta — released pins

Freeze never changes reachability, only control (rules/30-freeze.md). One of the three ways that yields a new capture: freezing an enemy pinner frees the piece it was pinning.

**Files:**
- Modify: `crates/core/src/spell_delta.rs`

**Interfaces:**
- Consumes: `Delta`, `slider_attacks`, `our_sliders` (Task 3); `pins_of`, `PinMap`, `in_baseline` (Task 2).
- Produces: freeze casts return `Complete` when the only mechanism in play is a released pin. No new public API.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/core/src/spell_delta.rs`:

```rust
    #[test]
    fn freezing_a_pinner_frees_the_pinned_piece_to_capture() {
        // Black Rd8 pins White Rd4 to Kd1 down the d-file. Rd4 cannot take the
        // undefended Be4 while pinned. freeze@c8 covers d8, killing the pin, so
        // Rd4xe4 becomes legal -- a capture only the freeze enables.
        let mut pos = Position { board: crate::board::Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("d1").unwrap(), Some(crate::types::Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d4").unwrap(), Some(crate::types::Piece { color: Color::White, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("e4").unwrap(), Some(crate::types::Piece { color: Color::Black, kind: PieceKind::Bishop }));
        pos.board.set(Square::from_str("d8").unwrap(), Some(crate::types::Piece { color: Color::Black, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("h8").unwrap(), Some(crate::types::Piece { color: Color::Black, kind: PieceKind::King }));

        let baseline = crate::legal::legal_moves(&pos);
        let d4e4 = PieceMove::quiet(Square::from_str("d4").unwrap(), Square::from_str("e4").unwrap());
        assert!(!baseline.contains(&d4e4), "Rd4xe4 must be pinned-illegal without the spell");

        let cast = SpellCast { kind: SpellKind::Freeze, square: Square::from_str("c8").unwrap() };
        let mut out = Vec::new();
        assert_eq!(captures_enabled_by(&pos, cast, &baseline, &mut out), Delta::Complete);
        assert!(out.contains(&d4e4), "freezing the pinner must enable Rd4xe4, got {out:?}");
    }
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `~/.cargo/bin/cargo test -p spellchess-core spell_delta::tests::freezing_a_pinner`
Expected: FAIL — `assertion failed: left == right`, `NeedsRescan` vs `Complete` (freeze still declines unconditionally).

- [ ] **Step 3: Implement the released-pin delta**

In `crates/core/src/spell_delta.rs`, route freeze and add the implementation:

```rust
        SpellKind::Freeze => freeze_captures(pos, cast.square, baseline, out),
```

```rust
/// Pseudo-legal capture destinations for the piece standing on `from`, ignoring
/// pins and check -- the caller filters those. `slider_occ` already has live jump
/// squares subtracted.
fn piece_capture_targets(pos: &Position, from: Square, slider_occ: Bitboard, enemy_bb: Bitboard) -> Bitboard {
    let Some(piece) = pos.board.get(from) else { return Bitboard::EMPTY };
    let idx = from.0 as usize;
    let raw = match piece.kind {
        PieceKind::Knight => crate::rays::KNIGHT_ATTACKS[idx],
        PieceKind::King => crate::rays::KING_ATTACKS[idx],
        PieceKind::Pawn => crate::rays::PAWN_ATTACKS[piece.color.index()][idx],
        kind => slider_attacks(kind, from, slider_occ),
    };
    raw.intersect(enemy_bb)
}

/// Freeze never changes reachability, only control (rules/30-freeze.md: frozen
/// pieces "exert no control" but "still block sliding pieces"). So it can only hand
/// us a capture by dispelling a check, removing a pinner, or removing a defender.
fn freeze_captures(
    pos: &Position,
    s: Square,
    baseline: &[PieceMove],
    out: &mut Vec<PieceMove>,
) -> Delta {
    let us = pos.side_to_move;
    let enemy = us.opposite();
    let Some(king_sq) = pos.board.king_square(us) else {
        return Delta::NeedsRescan;
    };

    // En passant is a capture and interacts with freeze in ways this fast path
    // does not model; the rescan already special-cases it.
    if pos.en_passant.is_some() {
        return Delta::NeedsRescan;
    }

    let frozen_before = crate::spells::frozen_bb(pos);
    let zone = crate::spells::FREEZE_ZONE[s.0 as usize];
    let frozen_after = frozen_before.union(zone);
    let jump = crate::spells::jump_bb(pos);
    let occ = pos.board.occupancy();
    let slider_occ = occ.minus(jump);
    let enemy_bb = pos.board.color_bb(enemy);

    // Freezing our own pieces only removes our moves; `move_survives_own_freeze`
    // in legal.rs already handles that and this function must not model it.
    let checkers_before = crate::attacks::attackers_to(pos, king_sq, enemy, frozen_before, slider_occ);
    let checkers_after = crate::attacks::attackers_to(pos, king_sq, enemy, frozen_after, slider_occ);
    if !checkers_before.is_empty() || !checkers_after.is_empty() {
        // Check dispelled (or still live): the newly legal set is essentially the
        // whole position, which is not a cheap delta. This is what the escape
        // hatch exists for.
        return Delta::NeedsRescan;
    }

    let pins_before = pins_of(pos, king_sq, us, frozen_before, jump);
    let pins_after = pins_of(pos, king_sq, us, frozen_after, jump);
    let released = pins_before.pinned.minus(pins_after.pinned);

    for p in released.iter() {
        // Freeze hits our own pieces too (rules/30-freeze.md: "every piece in the
        // zone regardless of owner"). A piece released from a pin but caught in our
        // own zone has zero legal moves, so it must not contribute captures.
        if zone.contains(p) {
            continue;
        }
        let piece = pos.board.get(p).expect("pinned square is occupied");
        // Pawn captures carry promotion and en-passant variants; not worth
        // modelling here for a case this rare.
        if piece.kind == PieceKind::Pawn {
            return Delta::NeedsRescan;
        }
        for t in piece_capture_targets(pos, p, slider_occ, enemy_bb).iter() {
            if pos.board.get(t).is_some_and(|q| q.kind == PieceKind::King) {
                return Delta::NeedsRescan;
            }
            let mv = PieceMove::quiet(p, t);
            if in_baseline(baseline, mv) {
                continue;
            }
            out.push(mv);
        }
    }
    Delta::Complete
}
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
~/.cargo/bin/cargo test -p spellchess-core spell_delta
~/.cargo/bin/cargo test -p spellchess-core --test spell_delta_soundness -- --nocapture
```

Expected: both PASS, with a non-zero `complete` count now printed for freeze too.

- [ ] **Step 5: Run the full verification set**

```bash
~/.cargo/bin/cargo test --workspace
~/.cargo/bin/cargo test -p spellchess-search --release --test search_identity
~/.cargo/bin/cargo test -p spellchess-search --release -- --ignored
```

Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/spell_delta.rs
git commit -m "perf(core): compute freeze-released-pin captures by delta"
```

---

### Task 5: Freeze delta — removed defenders

The second freeze mechanism: freezing the last unfrozen defender of an enemy piece adjacent to our king lets the king capture it.

**Files:**
- Modify: `crates/core/src/spell_delta.rs`

**Interfaces:**
- Consumes: `freeze_captures` (Task 4), `king_dest_safe`, `position_with_field` (Task 2).
- Produces: no new public API.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/core/src/spell_delta.rs`:

```rust
    #[test]
    fn freezing_the_last_defender_lets_the_king_capture() {
        // Black Nd2 sits next to White Ke1, defended only by Ra2. Kxd2 is illegal.
        // freeze@a3 covers a2, so the knight is undefended and Kxd2 becomes legal.
        let mut pos = Position { board: crate::board::Board::empty(), ..Position::starting() };
        pos.board.set(Square::from_str("e1").unwrap(), Some(crate::types::Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(Square::from_str("d2").unwrap(), Some(crate::types::Piece { color: Color::Black, kind: PieceKind::Knight }));
        pos.board.set(Square::from_str("a2").unwrap(), Some(crate::types::Piece { color: Color::Black, kind: PieceKind::Rook }));
        pos.board.set(Square::from_str("h8").unwrap(), Some(crate::types::Piece { color: Color::Black, kind: PieceKind::King }));

        let baseline = crate::legal::legal_moves(&pos);
        let kxd2 = PieceMove::quiet(Square::from_str("e1").unwrap(), Square::from_str("d2").unwrap());
        assert!(!baseline.contains(&kxd2), "Kxd2 must be illegal while Ra2 defends d2");

        let cast = SpellCast { kind: SpellKind::Freeze, square: Square::from_str("a3").unwrap() };
        let mut out = Vec::new();
        assert_eq!(captures_enabled_by(&pos, cast, &baseline, &mut out), Delta::Complete);
        assert!(out.contains(&kxd2), "freezing the defender must enable Kxd2, got {out:?}");
    }
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `~/.cargo/bin/cargo test -p spellchess-core spell_delta::tests::freezing_the_last_defender`
Expected: FAIL — `out` does not contain `Kxd2` (Task 4 only emits released-pin captures).

- [ ] **Step 3: Implement the removed-defender delta**

In `freeze_captures`, insert this block immediately before the closing `Delta::Complete`:

```rust
    // Freezing an enemy piece removes its control, so an enemy piece beside our
    // king that only it defended becomes capturable. Legality is checked against
    // the hypothetical, which carries the new freeze field.
    //
    // Skipped entirely when our own king is inside our own freeze zone: a frozen
    // king has zero legal moves, including while in check (rules/30-freeze.md,
    // sacredRoyal off is the canonical setting).
    if !zone.contains(king_sq) {
        let hypo = crate::legal::position_with_field(pos, SpellCast { kind: SpellKind::Freeze, square: s });
        for t in crate::rays::KING_ATTACKS[king_sq.0 as usize].intersect(enemy_bb).iter() {
            if pos.board.get(t).is_some_and(|q| q.kind == PieceKind::King) {
                return Delta::NeedsRescan;
            }
            let mv = PieceMove::quiet(king_sq, t);
            if in_baseline(baseline, mv) {
                continue;
            }
            if crate::legal::king_dest_safe(&hypo, king_sq, t, enemy) {
                out.push(mv);
            }
        }
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
~/.cargo/bin/cargo test -p spellchess-core spell_delta
~/.cargo/bin/cargo test -p spellchess-core --test spell_delta_soundness -- --nocapture
```

Expected: both PASS. The differential test is the real gate here — it will catch a double-push of the same move, or a king capture emitted onto a square that is still defended.

- [ ] **Step 5: Run the full verification set**

```bash
~/.cargo/bin/cargo test --workspace
~/.cargo/bin/cargo test -p spellchess-search --release --test search_identity
~/.cargo/bin/cargo test -p spellchess-search --release -- --ignored
```

Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/spell_delta.rs
git commit -m "perf(core): compute freeze-removed-defender king captures by delta"
```

---

### Task 6: Final measurement and honest perf guards

Replace the perf assertions that have been silently false since `55b3f3e`, and record the real result in the spec.

**Files:**
- Modify: `crates/search/src/search.rs:723-776` (the `#[ignore]`d perf test)
- Modify: `docs/superpowers/specs/2026-09-01-spell-capture-delta-design.md` (Measurement section)

**Interfaces:**
- Consumes: everything above.
- Produces: nothing consumed by later tasks — this is the last one.

- [ ] **Step 1: Measure the final state**

```bash
~/.cargo/bin/cargo run --release -p spellchess-search --example hotcost
~/.cargo/bin/cargo build --release -p spellchess-cli
for d in 6 8; do
  echo "--- depth $d ---"
  SPELLCHESS_PROFILE=1 ./target/release/spellchess <<EOF
go --depth $d
quit
EOF
done
```

Record the depth-6 and depth-8 wall times and the `generate_quiescence_turns_from` ns/call. Compare against the Baseline table at the top of this plan.

- [ ] **Step 2: Rewrite the perf guard with measured bounds**

In `crates/search/src/search.rs`, the test `depth_budget_stays_bounded_on_a_realistic_board` currently asserts depth 15 finishes in 15s, which is false by roughly two orders of magnitude. Replace its depth-15 block with a depth-8 block bounded by the number just measured plus ~50% headroom, and update the doc comment to name the machine:

```rust
    /// Regression guard for search speed on the starting position.
    /// Depths 1, 3, 4, 6 and 8 share this test so they cannot run in parallel and
    /// contend for the same cores. Bounds are tuned for a release build on a
    /// Raspberry Pi 5 (odysseus) and measured 2026-09-01 after the spell-delta
    /// rewrite; a debug build's overhead swamps the algorithmic win. Run with
    /// `cargo test -p spellchess-search --release -- --ignored`.
```

Delete the depth-15 case entirely rather than re-tuning it — depth 15 is not reachable in a practical time, and an aspirational bound is exactly what let this rot unnoticed.

Then set each remaining bound from Step 1's measurements plus ~50% headroom, reusing the block shape already in the file. For example, if depth 6 measured 3.4s, its block becomes:

```rust
        let start = std::time::Instant::now();
        let result = search(&pos, Budget::Depth(6));
        let elapsed = start.elapsed();
        assert!(result.is_some(), "a legal turn exists in the starting position");
        assert!(
            elapsed < Duration::from_secs(5),
            "depth-6 search on the starting position must finish in under 5s, took {elapsed:?}",
        );
```

Add an equivalent block for depth 8 using its measured time plus headroom, and update the depth 1/3/4 bounds the same way.

- [ ] **Step 3: Verify the guard passes and is honest**

Run: `~/.cargo/bin/cargo test -p spellchess-search --release -- --ignored --nocapture`
Expected: PASS, with printed timings comfortably inside each bound. If any bound is met only barely, raise that bound — a flaky perf guard gets ignored, which is the failure mode being fixed.

- [ ] **Step 4: Record the result in the spec**

In `docs/superpowers/specs/2026-09-01-spell-capture-delta-design.md`, add a row to the Measurement table's right-hand side with the achieved numbers, and one sentence stating whether the ≤4s / ≤90s targets were met. If they were not met, state the actual figure and the remaining hot spot from `hotcost` — do not quietly restate the target as the result.

- [ ] **Step 5: Full green run**

```bash
~/.cargo/bin/cargo test --workspace
~/.cargo/bin/cargo test -p spellchess-search --release -- --ignored
~/.cargo/bin/cargo clippy --workspace --all-targets
```

Expected: tests PASS; clippy clean of new warnings in `spell_delta.rs`.

- [ ] **Step 6: Commit**

```bash
git add crates/search/src/search.rs docs/superpowers/specs/2026-09-01-spell-capture-delta-design.md
git commit -m "test(search): replace the stale depth-15 perf guard with measured bounds"
```

---

## Notes for the implementer

- **The oracle is always right.** When `spell_delta_soundness` reports a mismatch, the rescan is correct by definition and the delta is wrong. Reproduce the single failing `SpellCast` before editing anything, and never weaken the assertion to get green.
- **`NeedsRescan` is free.** If a case is subtle, decline. A declined case costs one rescan; a wrong `Complete` silently corrupts the search. Prefer declining and noting it over guessing.
- **This codebase's spell interactions are repeatedly subtler than they look.** Differential fuzzing has caught real bugs that survived careful manual review three separate times (jump relevance gaps, the king-capture `attacker_count` rule, the item-9 quiescence regression). Trust the fuzz harness over your reading of the rules.
- **Rules questions go to `rules/INDEX.md` first**, which routes to the exact file and anchor. Statements are tagged `[VERIFIED]` / `[CODE]` / `[DOC]` / `[UNVERIFIED]`; when code and prose disagree, code wins.

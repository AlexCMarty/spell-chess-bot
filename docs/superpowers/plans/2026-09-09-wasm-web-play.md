# Browser WASM Play Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a static GitHub Pages site where a visitor plays Spell Chess against this engine compiled to WebAssembly, with a live analysis panel and no server component.

**Architecture:** A new `crates/wasm` crate exposes a stateful `Game` over `wasm-bindgen`. That module runs inside a Web Worker which owns the authoritative `Position`; the main thread is a pure view that sends commands and renders returned JSON. Three small changes land in existing crates: a cfg-gated clock so `Instant` works on wasm, a progress callback on the search, and `notation.rs` moving from `cli` to `core`.

**Tech Stack:** Rust, `wasm-bindgen` 0.2, `wasm-pack` 0.15, `web-time` (wasm only), vanilla ES-module JavaScript (no bundler, no npm), GitHub Actions + `actions/deploy-pages`.

**Spec:** `docs/superpowers/specs/2026-09-09-wasm-web-play-design.md`

## Global Constraints

- **Node and qnode counts must stay bit-identical** across every change in this plan. If they move, the search tree changed — that is a correctness review, not a benchmark. Verify with `cargo run --release -p spellchess-search --example bench`.
- **Never delete the slow rescan path.** Nothing in this plan touches `spell_delta.rs`; if a task seems to need to, stop and escalate.
- **Conventional Commits are mandatory** for every commit (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`).
- **`cargo` may not be on `PATH`** — use `~/.cargo/bin/cargo` if plain `cargo` fails. Release builds take a couple of minutes; budget long timeouts rather than assuming a hang.
- **`web-time` is a wasm-only dependency.** It must appear under `[target.'cfg(target_arch = "wasm32")'.dependencies]` only. Native builds of `core`, `search`, and `cli` keep their current zero non-dev dependencies.
- **All asset paths in `web/` must be relative.** The site is served from the repo subpath `https://alexcmarty.github.io/spell-chess-bot/`; a leading `/` breaks it.
- **Square index convention:** `Square(rank * 8 + file)`, so index 0 is a1 and index 63 is h8. `Square` implements `Display` as algebraic notation, and `Square::from_str("e4") -> Option<Square>` parses it back.
- **Existing suites must stay green:** `cargo test --workspace` after every task.

## Browser Verification

Tasks 7, 8, and 10 say "open it in a browser and walk this checklist." The driver
is the **`open-claude-in-chrome` MCP**, verified operational on this machine
against a local static server. What was confirmed, and the three traps found:

- **Module workers, `WebAssembly.instantiate`, and `wasm-pack --target web`
  output all load and run** from `python3 -m http.server -d web 8099`. No flags,
  no HTTPS needed for `localhost`.
- **`computer` screenshots fail with `CDP Page.captureScreenshot timed out`
  unless the tab is the active tab in its window.** Call `set_tab_focus` once
  first; after that, screenshots and coordinate clicks work.
- **`find` does not see the board.** Task 6 renders squares as
  `<div data-square="e2">` — no interactive role, so natural-language element
  search returns nothing. Drive the board with `javascript_tool` instead:
  `document.querySelector('[data-square="e2"]').click()`. A programmatic
  `.click()` bubbles, so Task 6's delegated listener fires correctly. This is
  more reliable than coordinate clicks and needs no screenshot.
- **`javascript_tool` reports thrown errors as a bare `Uncaught`** with no
  message. If a snippet fails, bisect it into single expressions rather than
  guessing — a `null` from `querySelector` looks identical to a tool failure.
- `read_console_messages` (with `onlyErrors: true`) covers the "console is free
  of errors" checks, and `debug` with `kind: "hit"` confirms what a click
  actually landed on.

## File Structure

| Path | Responsibility |
|---|---|
| `crates/core/src/notation.rs` | **Moved from `cli`.** `parse_turn` / `format_turn`. Shared `Turn ↔ String` vocabulary. |
| `crates/cli/src/notation.rs` | **Reduced to a re-export** so CLI callers and tests are untouched. |
| `crates/search/src/clock.rs` | **New.** Two-line cfg switch selecting `web_time` on wasm, `std::time` elsewhere. |
| `crates/search/src/search.rs` | **Modified.** `search_with_progress` added; `iterate` gains a callback parameter. |
| `crates/wasm/src/json.rs` | **New.** Minimal JSON string builder. No serde — keeps the wasm binary small. |
| `crates/wasm/src/view.rs` | **New.** Pure functions turning `Position`/`Turn` into JSON and parsing boundary arguments. No `wasm_bindgen` types, so it is fully testable on the native target. |
| `crates/wasm/src/lib.rs` | **New.** The `#[wasm_bindgen] Game` shell. Thin — delegates to `view.rs`. |
| `web/clockbench.html` | **New.** Throwaway measurement page for Task 5's deadline-check overhead. Not part of the site. |
| `web/index.html` | **New.** Page skeleton. |
| `web/style.css` | **New.** Board, spell tray, panel styling. |
| `web/app.js` | **New.** View rendering and input handling. Holds no authoritative game state. |
| `web/worker.js` | **New.** Module worker. Loads the wasm, owns the `Game`, speaks the command protocol. |
| `.github/workflows/pages.yml` | **New.** Build wasm, upload `web/`, deploy to Pages. |

---

### Task 1: Move `notation.rs` from `cli` into `core`

Moving this first means every later task can format and parse turns without the wasm crate depending on the CLI crate.

**Files:**
- Create: `crates/core/src/notation.rs` (content moved from `crates/cli/src/notation.rs`)
- Modify: `crates/core/src/lib.rs`
- Modify: `crates/cli/src/notation.rs` (replaced with a re-export)

**Interfaces:**
- Consumes: nothing.
- Produces: `spellchess_core::notation::{parse_turn, format_turn}`, re-exported at the crate root as `spellchess_core::{parse_turn, format_turn}`.
  - `pub fn parse_turn(input: &str, pos: &Position) -> Result<Turn, String>`
  - `pub fn format_turn(t: &Turn) -> String`

- [ ] **Step 1: Move the file**

```bash
cd "/home/alex/Spell Chess"
git mv crates/cli/src/notation.rs crates/core/src/notation.rs
```

- [ ] **Step 2: Rewrite the crate-internal imports**

The file currently reaches for its own crate by external name. Inside `core` that is `crate`. There are exactly two places: the top-level `use` on line 1, and the `use` inside `mod tests`.

```bash
cd "/home/alex/Spell Chess"
sed -i 's/^use spellchess_core::/use crate::/' crates/core/src/notation.rs
sed -i 's/^    use spellchess_core::/    use crate::/' crates/core/src/notation.rs
grep -n "spellchess_core" crates/core/src/notation.rs
```

Expected: the `grep` prints nothing. If it prints anything, fix those lines by hand — the file must not name its own crate externally.

- [ ] **Step 3: Register the module in `core`**

In `crates/core/src/lib.rs`, add after the `pub mod movegen;` / `pub use movegen::...` block:

```rust
pub mod notation;
pub use notation::{parse_turn, format_turn};
```

- [ ] **Step 4: Replace the CLI module with a re-export**

Write `crates/cli/src/notation.rs` containing exactly:

```rust
//! Turn notation moved to `spellchess-core` so the WASM front end can format and
//! parse turns without depending on this crate. Re-exported here so CLI callers
//! keep working against `crate::notation`.
pub use spellchess_core::notation::{format_turn, parse_turn};
```

- [ ] **Step 5: Run the full suite**

```bash
cd "/home/alex/Spell Chess" && ~/.cargo/bin/cargo test --workspace 2>&1 | tail -30
```

Expected: PASS. The notation round-trip tests now run under `spellchess-core` instead of `spellchess-cli`; the total test count across the workspace is unchanged.

- [ ] **Step 6: Commit**

```bash
cd "/home/alex/Spell Chess"
git add crates/core/src/notation.rs crates/core/src/lib.rs crates/cli/src/notation.rs
git commit -m "refactor: move turn notation from cli into core

Turn <-> string is shared vocabulary, not CLI presentation. Moving it lets
the forthcoming WASM front end format turns without depending on the CLI
crate. The old path stays as a re-export so CLI callers are untouched."
```

---

### Task 2: cfg-gated clock so `search` compiles and runs on wasm

`search_smp` calls `Instant::now()` on its first line, before it branches on `Budget`. On `wasm32-unknown-unknown` `std::time::Instant::now()` panics, so this blocks *any* search in the browser, depth-limited or not.

**Files:**
- Create: `crates/search/src/clock.rs`
- Modify: `crates/search/src/lib.rs`
- Modify: `crates/search/src/search.rs:2` (the `use std::time::{Duration, Instant};` line)
- Modify: `crates/search/Cargo.toml`

**Interfaces:**
- Consumes: nothing.
- Produces: `spellchess_search::clock::{Duration, Instant}`. On every target, `Duration` is exactly `std::time::Duration`, so `Budget::Time(std::time::Duration::from_secs(5))` keeps compiling for existing callers such as `crates/cli/src/repl.rs`.

- [ ] **Step 1: Add the wasm-only dependency**

Append to `crates/search/Cargo.toml`:

```toml
# Only on wasm: `std::time::Instant::now()` panics on wasm32-unknown-unknown, and
# `search_smp` calls it unconditionally. `web-time` backs `Instant` with the
# browser's Performance API and re-exports `std::time` verbatim everywhere else,
# so native builds keep their zero non-dev dependencies.
[target.'cfg(target_arch = "wasm32")'.dependencies]
web-time = "1"
```

- [ ] **Step 2: Write the clock module**

Create `crates/search/src/clock.rs`:

```rust
//! `Instant` that works in a browser.
//!
//! `std::time::Instant::now()` panics on `wasm32-unknown-unknown`, and
//! `search_smp` calls it on its first line regardless of budget. `web_time`
//! re-exports `std::time` unchanged off wasm, so this switch costs native
//! builds nothing -- not even a dependency, since it is target-gated.
//!
//! `Duration` is re-exported alongside `Instant` and is `std::time::Duration`
//! on every target, so `Budget::Time` stays interchangeable with std durations
//! built by callers.

#[cfg(target_arch = "wasm32")]
pub use web_time::{Duration, Instant};

#[cfg(not(target_arch = "wasm32"))]
pub use std::time::{Duration, Instant};
```

- [ ] **Step 3: Register the module**

In `crates/search/src/lib.rs`, add `pub mod clock;` as the first module line.

- [ ] **Step 4: Point `search.rs` at it**

In `crates/search/src/search.rs`, replace line 2:

```rust
use std::time::{Duration, Instant};
```

with:

```rust
use crate::clock::{Duration, Instant};
```

Then check for any remaining direct references, which must also go through the shim:

```bash
cd "/home/alex/Spell Chess" && grep -n "std::time::Instant" crates/search/src/search.rs
```

Expected: several hits inside `#[cfg(test)] mod tests` (lines ~860 onward). Those are native-only test timers and may stay as `std::time::Instant` — tests never run on wasm. Leave them.

- [ ] **Step 5: Verify native behaviour is unchanged**

```bash
cd "/home/alex/Spell Chess" && ~/.cargo/bin/cargo test --workspace 2>&1 | tail -20
```

Expected: PASS.

- [ ] **Step 6: Verify the search crate now builds for wasm**

```bash
cd "/home/alex/Spell Chess" && ~/.cargo/bin/cargo build -p spellchess-search --target wasm32-unknown-unknown --release 2>&1 | tail -20
```

Expected: `Finished` with no errors. This is the first proof the engine can exist in a browser at all.

- [ ] **Step 7: Commit**

```bash
cd "/home/alex/Spell Chess"
git add crates/search/Cargo.toml crates/search/src/clock.rs crates/search/src/lib.rs crates/search/src/search.rs Cargo.lock
git commit -m "feat(search): cfg-gated clock so the engine builds and runs on wasm

std::time::Instant::now() panics on wasm32-unknown-unknown and search_smp
calls it unconditionally. web-time is target-gated to wasm and re-exports
std::time elsewhere, so native builds are bit-identical and keep zero
non-dev dependencies."
```

---

### Task 3: `search_with_progress` for the live analysis panel

The analysis panel needs per-depth updates while the engine thinks. `search_smp` only returns a final answer.

This adds a callback to the iterative-deepening loop. It is an API addition, **not** a change to the search tree — the node-count check in Step 6 is the gate that proves it.

**Files:**
- Modify: `crates/search/src/search.rs` (`iterate` signature and body, new `search_with_progress`, `search_smp` body)
- Test: `crates/search/src/search.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `spellchess_search::clock::Instant` from Task 2.
- Produces:
  - `pub fn search_with_progress(pos: &Position, budget: Budget, threads: usize, on_iteration: &mut dyn FnMut(u32, i32, &Turn)) -> Option<(Turn, i32)>`
  - `search_smp(pos, budget, threads)` keeps its exact current signature and behaviour.

- [ ] **Step 1: Write the failing test**

Add to `crates/search/src/search.rs` inside `#[cfg(test)] mod tests`:

```rust
/// The analysis panel is driven entirely by this callback, so it must fire once
/// per completed iteration with strictly increasing depths, and the last thing
/// it reports must be the answer the search actually returns. A callback that
/// lagged the return value by an iteration would render a stale best move.
#[test]
fn progress_callback_reports_each_depth_and_ends_on_the_final_answer() {
    let pos = Position::starting();
    let mut seen: Vec<(u32, i32, Turn)> = Vec::new();
    let got = search_with_progress(&pos, Budget::Depth(4), 1, &mut |depth, score, turn| {
        seen.push((depth, score, *turn));
    });

    assert!(!seen.is_empty(), "callback never fired");
    let depths: Vec<u32> = seen.iter().map(|(d, _, _)| *d).collect();
    assert!(
        depths.windows(2).all(|w| w[1] > w[0]),
        "depths must strictly increase, got {depths:?}"
    );
    assert_eq!(*depths.last().unwrap(), 4, "last iteration should be the full depth");

    let (turn, score) = got.expect("search must find a turn from the starting position");
    let (_, last_score, last_turn) = seen.last().unwrap();
    assert_eq!(*last_turn, turn, "final callback turn must match the returned turn");
    assert_eq!(*last_score, score, "final callback score must match the returned score");
}

/// `search_smp` must be exactly `search_with_progress` with a no-op callback.
/// If the two ever diverge, the CLI and the browser are running different engines.
#[test]
fn search_smp_matches_search_with_progress() {
    let pos = Position::starting();
    let a = search_smp(&pos, Budget::Depth(4), 1);
    let b = search_with_progress(&pos, Budget::Depth(4), 1, &mut |_, _, _| {});
    assert_eq!(a, b, "single-threaded search must be deterministic and identical");
}
```

- [ ] **Step 2: Run to verify it fails**

```bash
cd "/home/alex/Spell Chess" && ~/.cargo/bin/cargo test -p spellchess-search progress_callback 2>&1 | tail -20
```

Expected: FAIL — `cannot find function search_with_progress in this scope`.

- [ ] **Step 3: Thread the callback through `iterate`**

In `crates/search/src/search.rs`, change the `iterate` signature (currently at line 564) to take the callback as its final parameter:

```rust
fn iterate(
    pos: &Position,
    max_depth: u32,
    start_depth: u32,
    aspiration: bool,
    state: &mut SearchState<'_>,
    on_iteration: &mut dyn FnMut(u32, i32, &Turn),
) -> Option<(Turn, i32)> {
```

The callback deliberately does **not** live on `SearchState`: that struct is shared with helper threads and must stay `Send`, while `&mut dyn FnMut` is not.

Inside the loop, immediately after the existing `best = Some((turn, score));` line, report the completed iteration:

```rust
                        best = Some((turn, score));
                        on_iteration(depth, score, &turn);
```

- [ ] **Step 4: Add `search_with_progress` and re-point `search_smp`**

Rename the existing `pub fn search_smp` body to `search_with_progress` by changing its signature to:

```rust
pub fn search_with_progress(
    pos: &Position,
    budget: Budget,
    threads: usize,
    on_iteration: &mut dyn FnMut(u32, i32, &Turn),
) -> Option<(Turn, i32)> {
```

Keep the existing doc comment on it. Inside that body there are two `iterate` call sites to update:

The helper-thread call (inside `scope.spawn`) gets its own no-op, because the real callback is not `Send`:

```rust
                    iterate(pos, max_depth, 1 + offset, false, &mut state, &mut |_, _, _| {});
```

The main-thread call passes the caller's callback through:

```rust
        best = iterate(pos, max_depth, 1, true, &mut state, on_iteration);
```

Then add the thin wrapper immediately after that function:

```rust
/// `search_with_progress` with nothing listening. Every existing caller wants
/// this; only the browser front end, which renders a live analysis panel, needs
/// the per-iteration reports.
pub fn search_smp(pos: &Position, budget: Budget, threads: usize) -> Option<(Turn, i32)> {
    search_with_progress(pos, budget, threads, &mut |_, _, _| {})
}
```

- [ ] **Step 5: Run the tests**

```bash
cd "/home/alex/Spell Chess" && ~/.cargo/bin/cargo test --workspace 2>&1 | tail -20
```

Expected: PASS, including the two new tests and the existing `search_identity` suite.

- [ ] **Step 6: Verify the node-count invariant**

This is the gate. Record the counts before and after; they must be identical.

```bash
cd "/home/alex/Spell Chess"
SPELLCHESS_PROFILE=1 ~/.cargo/bin/cargo run --release -p spellchess-search --example bench 2>&1 | tail -20
```

Compare `nodes` and `qnodes` against the same command run on the previous commit (`git stash` or a scratch worktree). Expected: **bit-identical**. If they differ, the callback was placed somewhere that alters control flow — revert Step 3 and re-place it. Do not proceed with differing counts.

- [ ] **Step 7: Commit**

```bash
cd "/home/alex/Spell Chess"
git add crates/search/src/search.rs
git commit -m "feat(search): report each completed iteration via a callback

search_with_progress threads a per-iteration callback through iterative
deepening so the browser front end can render a live analysis panel.
search_smp is now that call with a no-op callback, so every existing caller
is unchanged. Node and qnode counts verified bit-identical."
```

---

### Task 4: The JSON view layer, testable natively

Everything that turns engine types into strings lives here, in plain Rust with no `wasm_bindgen` in any signature, so the whole layer is covered by `cargo test` without a browser.

**Files:**
- Create: `crates/wasm/Cargo.toml`
- Create: `crates/wasm/src/json.rs`
- Create: `crates/wasm/src/view.rs`
- Create: `crates/wasm/src/lib.rs` (module declarations only for now)
- Modify: `Cargo.toml` (workspace members)

**Interfaces:**
- Consumes: `spellchess_core::{format_turn, generate_turns}` from Task 1.
- Produces, all in `crate::view`:
  - `pub fn state_json(pos: &Position) -> String`
  - `pub fn turns_json(turns: &[Turn]) -> String`
  - `pub fn parse_square(s: &str) -> Result<Square, String>`
  - `pub fn parse_promo(s: Option<&str>) -> Result<Option<Promotion>, String>`
  - `pub fn parse_spell(kind: Option<&str>, at: Option<&str>) -> Result<Option<SpellCast>, String>`
  - `pub fn find_turn(pos: &Position, from: Square, to: Square, promo: Option<Promotion>, spell: Option<SpellCast>) -> Result<Turn, String>`

- [ ] **Step 1: Register the crate in the workspace**

In the root `Cargo.toml`, change the members line to:

```toml
members = ["crates/core", "crates/search", "crates/cli", "crates/wasm"]
```

Create `crates/wasm/Cargo.toml`:

```toml
[package]
name = "spellchess-wasm"
version = "0.1.0"
edition = "2021"

[lib]
# cdylib for the browser; rlib so `cargo test` can reach the view layer natively.
crate-type = ["cdylib", "rlib"]

[dependencies]
spellchess-core = { path = "../core" }
spellchess-search = { path = "../search" }

[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen = "0.2"
js-sys = "0.3"
console_error_panic_hook = "0.1"
```

Create `crates/wasm/src/lib.rs`:

```rust
mod json;
mod view;
```

- [ ] **Step 2: Write the failing tests**

Create `crates/wasm/src/view.rs` with only its test module to start:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use spellchess_core::{apply_turn, parse_turn, Position};

    #[test]
    fn starting_position_serializes_board_and_spells() {
        let out = state_json(&Position::starting());
        // Index 0 is a1, index 63 is h8 -- Square is rank * 8 + file.
        assert!(out.contains(r#""board":["wR","wN","wB","wQ","wK","wB","wN","wR","wP"#), "got {out}");
        assert!(out.contains(r#""sideToMove":"white""#), "got {out}");
        assert!(out.contains(r#""freeze":{"count":5,"lock":0}"#), "got {out}");
        assert!(out.contains(r#""jump":{"count":2,"lock":0}"#), "got {out}");
        assert!(out.contains(r#""fields":[]"#), "got {out}");
        assert!(out.contains(r#""status":{"kind":"inProgress"}"#), "got {out}");
    }

    /// A cast spell must show up as a live field, because the board tints frozen
    /// squares from exactly this list. If it were dropped, the UI would silently
    /// stop showing that pieces are frozen.
    #[test]
    fn a_cast_spell_appears_in_fields() {
        let pos = Position::starting();
        let turn = parse_turn("freeze@e6 e2e4", &pos).expect("freeze@e6 e2e4 must be legal");
        let next = apply_turn(&pos, &turn);
        let out = state_json(&next);
        assert!(out.contains(r#""square":"e6""#), "got {out}");
        assert!(out.contains(r#""kind":"freeze""#), "got {out}");
        assert!(out.contains(r#""owner":"white""#), "got {out}");
        assert!(out.contains(r#""sideToMove":"black""#), "got {out}");
    }

    #[test]
    fn turns_json_carries_squares_spell_and_display_text() {
        let pos = Position::starting();
        let turns = vec![parse_turn("freeze@e6 e2e4", &pos).unwrap()];
        let out = turns_json(&turns);
        assert_eq!(
            out,
            r#"[{"from":"e2","to":"e4","promo":null,"spell":{"kind":"freeze","at":"e6"},"text":"freeze@e6 e2e4"}]"#
        );
    }

    #[test]
    fn plain_turn_has_null_spell_and_promo() {
        let pos = Position::starting();
        let turns = vec![parse_turn("e2e4", &pos).unwrap()];
        let out = turns_json(&turns);
        assert_eq!(out, r#"[{"from":"e2","to":"e4","promo":null,"spell":null,"text":"e2e4"}]"#);
    }

    #[test]
    fn parse_square_rejects_junk() {
        assert_eq!(parse_square("e4").unwrap(), Square::from_str("e4").unwrap());
        assert!(parse_square("z9").is_err());
        assert!(parse_square("").is_err());
        assert!(parse_square("e44").is_err());
    }

    #[test]
    fn parse_spell_requires_both_halves() {
        assert!(parse_spell(None, None).unwrap().is_none());
        let cast = parse_spell(Some("freeze"), Some("e6")).unwrap().unwrap();
        assert_eq!(cast.kind, SpellKind::Freeze);
        assert_eq!(cast.square, Square::from_str("e6").unwrap());
        assert!(parse_spell(Some("freeze"), None).is_err(), "half a spell is a protocol bug");
        assert!(parse_spell(Some("fireball"), Some("e6")).is_err());
    }

    /// The UI only ever offers turns from `turns_json`, so reaching this error is a
    /// bug signal rather than a user mistake -- but it must be an error, never a
    /// silently-applied different turn.
    #[test]
    fn find_turn_rejects_an_illegal_turn() {
        let pos = Position::starting();
        let from = Square::from_str("e2").unwrap();
        let to = Square::from_str("e5").unwrap();
        assert!(find_turn(&pos, from, to, None, None).is_err());
    }

    #[test]
    fn find_turn_matches_the_spell_as_well_as_the_move() {
        let pos = Position::starting();
        let from = Square::from_str("e2").unwrap();
        let to = Square::from_str("e4").unwrap();
        let cast = SpellCast { kind: SpellKind::Freeze, square: Square::from_str("e6").unwrap() };
        let found = find_turn(&pos, from, to, None, Some(cast)).unwrap();
        assert_eq!(found.spell, Some(cast), "the staged cast must survive into the applied turn");
        let plain = find_turn(&pos, from, to, None, None).unwrap();
        assert_eq!(plain.spell, None);
    }
}
```

- [ ] **Step 3: Run to verify they fail**

```bash
cd "/home/alex/Spell Chess" && ~/.cargo/bin/cargo test -p spellchess-wasm 2>&1 | tail -20
```

Expected: FAIL to compile — `cannot find function state_json`.

- [ ] **Step 4: Write the JSON builder**

Create `crates/wasm/src/json.rs`:

```rust
//! A JSON writer sized to exactly what this crate emits.
//!
//! Everything crossing the boundary is either a fixed key, an algebraic square,
//! a small integer, or a word from a closed set -- none of which need escaping.
//! `escape` exists anyway so a future field carrying arbitrary text cannot
//! silently emit malformed JSON. Hand-rolling this instead of pulling in
//! serde_json keeps roughly 100KB out of the wasm binary.

/// Escapes the characters JSON forbids raw in a string. Control characters below
/// 0x20 become `\u00XX`.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// A quoted, escaped JSON string.
pub fn string(s: &str) -> String {
    format!("\"{}\"", escape(s))
}

/// `{"a":1,"b":2}` from pre-rendered values. Values are inserted verbatim, so
/// callers pass `string(..)` for text and plain `format!` output for numbers.
pub fn object(fields: &[(&str, String)]) -> String {
    let body: Vec<String> = fields.iter().map(|(k, v)| format!("{}:{}", string(k), v)).collect();
    format!("{{{}}}", body.join(","))
}

/// `[a,b,c]` from pre-rendered elements.
pub fn array(items: &[String]) -> String {
    format!("[{}]", items.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_quotes_backslashes_and_controls() {
        assert_eq!(escape(r#"a"b\c"#), r#"a\"b\\c"#);
        assert_eq!(escape("a\nb"), "a\\nb");
        assert_eq!(escape("a\u{1}b"), "a\\u0001b");
    }

    #[test]
    fn builds_objects_and_arrays() {
        assert_eq!(object(&[("a", "1".to_string()), ("b", string("x"))]), r#"{"a":1,"b":"x"}"#);
        assert_eq!(array(&["1".to_string(), "2".to_string()]), "[1,2]");
        assert_eq!(object(&[]), "{}");
        assert_eq!(array(&[]), "[]");
    }
}
```

- [ ] **Step 5: Write the view layer**

Prepend to `crates/wasm/src/view.rs`, above the existing test module:

```rust
//! Engine types in, JSON out -- and boundary arguments in, engine types out.
//!
//! Nothing here mentions `wasm_bindgen`, so every function in this file is
//! reachable from `cargo test` on the native target. `lib.rs` is the only
//! wasm-aware file in the crate, and it is a shell over this one.

use crate::json;
use spellchess_core::{
    format_turn, game_status, generate_turns, Color, GameStatus, PieceKind, Position, Promotion,
    SpellCast, SpellCounter, SpellKind, Square, Turn,
};

fn color_name(c: Color) -> &'static str {
    match c {
        Color::White => "white",
        Color::Black => "black",
    }
}

fn spell_name(k: SpellKind) -> &'static str {
    match k {
        SpellKind::Freeze => "freeze",
        SpellKind::Jump => "jump",
    }
}

/// `"wP"`, `"bK"` -- colour letter then the standard uppercase piece letter, so
/// the front end can map a square to a glyph with a single lookup.
fn piece_code(kind: PieceKind, color: Color) -> String {
    let k = match kind {
        PieceKind::Pawn => 'P',
        PieceKind::Knight => 'N',
        PieceKind::Bishop => 'B',
        PieceKind::Rook => 'R',
        PieceKind::Queen => 'Q',
        PieceKind::King => 'K',
    };
    let c = if color == Color::White { 'w' } else { 'b' };
    format!("{c}{k}")
}

fn promo_code(p: Promotion) -> &'static str {
    match p {
        Promotion::Queen => "q",
        Promotion::Rook => "r",
        Promotion::Bishop => "b",
        Promotion::Knight => "n",
    }
}

fn counter_json(c: SpellCounter) -> String {
    json::object(&[
        ("count", c.count.to_string()),
        ("lock", c.lock.to_string()),
    ])
}

fn status_json(pos: &Position) -> String {
    match game_status(pos) {
        GameStatus::InProgress => json::object(&[("kind", json::string("inProgress"))]),
        GameStatus::Stalemate => json::object(&[("kind", json::string("stalemate"))]),
        GameStatus::Checkmate(w) => json::object(&[
            ("kind", json::string("checkmate")),
            ("winner", json::string(color_name(w))),
        ]),
        GameStatus::KingCaptured(w) => json::object(&[
            ("kind", json::string("kingCaptured")),
            ("winner", json::string(color_name(w))),
        ]),
    }
}

/// The whole rendered state of a position, in the shape `app.js` draws from.
pub fn state_json(pos: &Position) -> String {
    // Index 0 is a1 and index 63 is h8, matching `Square(rank * 8 + file)`.
    let board: Vec<String> = (0..64u8)
        .map(|i| match pos.board.get(Square(i)) {
            Some(p) => json::string(&piece_code(p.kind, p.color)),
            None => json::string(""),
        })
        .collect();

    let spells = json::object(&[
        (
            "white",
            json::object(&[
                ("freeze", counter_json(pos.white_spells.freeze)),
                ("jump", counter_json(pos.white_spells.jump)),
            ]),
        ),
        (
            "black",
            json::object(&[
                ("freeze", counter_json(pos.black_spells.freeze)),
                ("jump", counter_json(pos.black_spells.jump)),
            ]),
        ),
    ]);

    let fields: Vec<String> = pos
        .fields
        .iter()
        .map(|f| {
            json::object(&[
                ("square", json::string(&f.square.to_string())),
                ("owner", json::string(color_name(f.owner))),
                ("kind", json::string(spell_name(f.kind))),
                ("expiresAfterPly", f.expires_after_ply.to_string()),
            ])
        })
        .collect();

    json::object(&[
        ("board", json::array(&board)),
        ("sideToMove", json::string(color_name(pos.side_to_move))),
        ("ply", pos.ply.to_string()),
        ("spells", spells),
        ("fields", json::array(&fields)),
        ("status", status_json(pos)),
    ])
}

/// One turn as the front end needs it: the squares to highlight, the staged
/// spell, and `text` for the move list.
pub fn turn_json(t: &Turn) -> String {
    let spell = match t.spell {
        Some(cast) => json::object(&[
            ("kind", json::string(spell_name(cast.kind))),
            ("at", json::string(&cast.square.to_string())),
        ]),
        None => "null".to_string(),
    };
    let promo = match t.mv.promotion {
        Some(p) => json::string(promo_code(p)),
        None => "null".to_string(),
    };
    json::object(&[
        ("from", json::string(&t.mv.from.to_string())),
        ("to", json::string(&t.mv.to.to_string())),
        ("promo", promo),
        ("spell", spell),
        ("text", json::string(&format_turn(t))),
    ])
}

pub fn turns_json(turns: &[Turn]) -> String {
    let items: Vec<String> = turns.iter().map(turn_json).collect();
    json::array(&items)
}

pub fn parse_square(s: &str) -> Result<Square, String> {
    Square::from_str(s).ok_or_else(|| format!("not a square: {s:?}"))
}

pub fn parse_promo(s: Option<&str>) -> Result<Option<Promotion>, String> {
    match s {
        None => Ok(None),
        Some("q") => Ok(Some(Promotion::Queen)),
        Some("r") => Ok(Some(Promotion::Rook)),
        Some("b") => Ok(Some(Promotion::Bishop)),
        Some("n") => Ok(Some(Promotion::Knight)),
        Some(other) => Err(format!("not a promotion piece: {other:?}")),
    }
}

/// Half a spell is a front-end bug, not a user action, so it is an error rather
/// than a silent "no spell" -- swallowing it would apply a different turn than
/// the one the user staged.
pub fn parse_spell(kind: Option<&str>, at: Option<&str>) -> Result<Option<SpellCast>, String> {
    match (kind, at) {
        (None, None) => Ok(None),
        (Some(k), Some(a)) => {
            let kind = match k {
                "freeze" => SpellKind::Freeze,
                "jump" => SpellKind::Jump,
                other => return Err(format!("not a spell: {other:?}")),
            };
            Ok(Some(SpellCast { kind, square: parse_square(a)? }))
        }
        _ => Err("a spell needs both a kind and a target square".to_string()),
    }
}

/// Resolves a described turn against the position's legal turns.
///
/// Matching against `generate_turns` -- the exhaustive oracle, not a search
/// generator -- rather than constructing a `Turn` directly is what makes it
/// impossible for the front end to apply something illegal. It also fills in the
/// `is_en_passant` / `is_castle` flags, which the boundary never sends.
pub fn find_turn(
    pos: &Position,
    from: Square,
    to: Square,
    promo: Option<Promotion>,
    spell: Option<SpellCast>,
) -> Result<Turn, String> {
    generate_turns(pos)
        .into_iter()
        .find(|t| t.mv.from == from && t.mv.to == to && t.mv.promotion == promo && t.spell == spell)
        .ok_or_else(|| format!("illegal turn: {from}{to}"))
}
```

- [ ] **Step 6: Run the tests**

```bash
cd "/home/alex/Spell Chess" && ~/.cargo/bin/cargo test -p spellchess-wasm 2>&1 | tail -25
```

Expected: PASS, all tests in `json` and `view`.

If `a_cast_spell_appears_in_fields` fails because `freeze@e6 e2e4` is not legal from the starting position, do **not** weaken the assertion — print the legal turns with `generate_turns(&Position::starting())` and pick a genuinely legal freeze turn, then update the expected `square` in both that test and `turns_json_carries_squares_spell_and_display_text`.

- [ ] **Step 7: Confirm the workspace is still green**

```bash
cd "/home/alex/Spell Chess" && ~/.cargo/bin/cargo test --workspace 2>&1 | tail -15
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
cd "/home/alex/Spell Chess"
git add Cargo.toml Cargo.lock crates/wasm
git commit -m "feat(wasm): add natively-testable JSON view layer

Board, spell counters, live fields, status and legal turns render to JSON
through a hand-rolled writer -- no serde, keeping the wasm binary small.
Nothing here names wasm_bindgen, so cargo test covers the whole layer on
the native target. find_turn resolves against generate_turns so the front
end cannot submit an illegal turn."
```

---

### Task 5: The `Game` wasm-bindgen shell

**Files:**
- Modify: `crates/wasm/src/lib.rs`
- Modify: `.gitignore`

**Interfaces:**
- Consumes: everything `view.rs` produces (Task 4); `spellchess_search::search::{search_with_progress, Budget}` (Task 3).
- Produces, as JavaScript on the `Game` class: `new Game()`, `reset()`, `undo() -> bool`, `state_json() -> string`, `legal_turns_json() -> string`, `apply_turn(from, to, promo, spell_kind, spell_at) -> string` (throws on error), `search(millis, on_progress) -> string`. Also a module-level `bench_clock(iters) -> number`, used only by the Step 5 overhead measurement.

- [ ] **Step 1: Write `lib.rs`**

Replace `crates/wasm/src/lib.rs` with:

```rust
//! The browser boundary. A shell over `view`: every method here parses its
//! arguments, calls one `view` function, and hands back a JSON string.
//!
//! Kept deliberately thin because nothing in this file can be reached by
//! `cargo test` -- the logic worth testing lives in `view.rs`, which can.

mod json;
mod view;

#[cfg(target_arch = "wasm32")]
mod bindings {
    use crate::view;
    use spellchess_core::{apply_turn, Position};
    use spellchess_search::clock::{Duration, Instant};
    use spellchess_search::search::{search_with_progress, Budget};
    use wasm_bindgen::prelude::*;

    /// Installed once at module load so a panic inside the engine surfaces as a
    /// readable console message instead of an opaque `unreachable executed`.
    #[wasm_bindgen(start)]
    pub fn start() {
        console_error_panic_hook::set_once();
    }

    /// Nanoseconds per `timed_out()` clock read.
    ///
    /// `Budget::Time` -- the mode the site uses -- leaves `deadline` as `Some`,
    /// so `Instant::now()` runs once per node *and* once per qnode. Natively
    /// that is a vDSO read; in the browser it is a JS boundary crossing through
    /// `performance.now()`. Step 5 sizes that against the search budget.
    ///
    /// Measurement only. It shares no state with the search and cannot move a
    /// node count.
    #[wasm_bindgen]
    pub fn bench_clock(iters: u32) -> f64 {
        let deadline = Instant::now() + Duration::from_secs(3600);
        let t0 = Instant::now();
        let mut hits = 0u32;
        for _ in 0..iters {
            // Deliberately the exact shape of `timed_out`'s second line.
            if Instant::now() >= deadline {
                hits += 1;
            }
        }
        let elapsed = t0.elapsed().as_nanos() as f64;
        // Keeps the loop observable so LLVM cannot delete it. The deadline is an
        // hour out, so this never returns -1.
        if hits > 0 {
            return -1.0;
        }
        elapsed / iters as f64
    }

    /// The authoritative game. One of these lives in the Web Worker; the main
    /// thread keeps no game state of its own.
    #[wasm_bindgen]
    pub struct Game {
        pos: Position,
        history: Vec<Position>,
    }

    #[wasm_bindgen]
    impl Game {
        #[wasm_bindgen(constructor)]
        pub fn new() -> Game {
            Game { pos: Position::starting(), history: Vec::new() }
        }

        pub fn reset(&mut self) {
            self.pos = Position::starting();
            self.history.clear();
        }

        /// Steps back one turn. Returns false when there is nothing to undo, so
        /// the UI can leave the button disabled rather than guess.
        pub fn undo(&mut self) -> bool {
            match self.history.pop() {
                Some(prev) => {
                    self.pos = prev;
                    true
                }
                None => false,
            }
        }

        pub fn state_json(&self) -> String {
            view::state_json(&self.pos)
        }

        pub fn legal_turns_json(&self) -> String {
            view::turns_json(&spellchess_core::generate_turns(&self.pos))
        }

        /// Applies a turn described by the boundary shapes. Returns the new state
        /// JSON. Errors become JavaScript exceptions: the UI only offers turns
        /// from `legal_turns_json`, so an error here means a front-end bug, and
        /// failing loudly beats applying something else.
        pub fn apply_turn(
            &mut self,
            from: &str,
            to: &str,
            promo: Option<String>,
            spell_kind: Option<String>,
            spell_at: Option<String>,
        ) -> Result<String, JsError> {
            let resolve = || -> Result<_, String> {
                let from = view::parse_square(from)?;
                let to = view::parse_square(to)?;
                let promo = view::parse_promo(promo.as_deref())?;
                let spell = view::parse_spell(spell_kind.as_deref(), spell_at.as_deref())?;
                view::find_turn(&self.pos, from, to, promo, spell)
            };
            let turn = resolve().map_err(|e| JsError::new(&e))?;
            self.history.push(self.pos.clone());
            self.pos = apply_turn(&self.pos, &turn);
            Ok(view::state_json(&self.pos))
        }

        /// Searches for `millis`, invoking `on_progress(depth, score, turn_json)`
        /// after each completed iteration. Returns the chosen turn as JSON, or
        /// `"null"` when the position has no legal turn.
        ///
        /// `threads` is hard-coded to 1: the browser cannot spawn them, and
        /// Lazy-SMP is a measured net loss on this engine anyway.
        pub fn search(&self, millis: u32, on_progress: &js_sys::Function) -> String {
            let budget = Budget::Time(Duration::from_millis(millis as u64));
            let mut report = |depth: u32, score: i32, turn: &spellchess_core::Turn| {
                let _ = on_progress.call3(
                    &JsValue::NULL,
                    &JsValue::from(depth),
                    &JsValue::from(score),
                    &JsValue::from_str(&view::turn_json(turn)),
                );
            };
            match search_with_progress(&self.pos, budget, 1, &mut report) {
                Some((turn, _)) => view::turn_json(&turn),
                None => "null".to_string(),
            }
        }
    }
}
```

- [ ] **Step 2: Ignore the generated package**

Append to `.gitignore`:

```
# wasm-pack output, rebuilt by CI on every deploy
web/pkg/
```

- [ ] **Step 3: Build the wasm package**

```bash
cd "/home/alex/Spell Chess" && ~/.cargo/bin/wasm-pack build crates/wasm --target web --out-dir ../../web/pkg --release 2>&1 | tail -25
```

Expected: `[INFO]: :-) Done`. `web/pkg/spellchess_wasm.js` and `web/pkg/spellchess_wasm_bg.wasm` now exist.

- [ ] **Step 4: Check the binary size**

```bash
cd "/home/alex/Spell Chess" && ls -lh web/pkg/spellchess_wasm_bg.wasm
```

Record the number. Anything under ~2MB is fine over the wire once gzipped. If it is dramatically larger, report it rather than adding optimisation flags on your own initiative.

- [ ] **Step 5: Measure the deadline-check overhead**

`Budget::Time` keeps `deadline` as `Some`, so `timed_out()` reads the clock once
per node and once per qnode. Natively that is a vDSO read; through
`wasm-bindgen` it is a call out to `performance.now()`. This step sizes it
before Task 10 discovers it as "the browser feels slow".

Create `web/clockbench.html` — a throwaway harness, not part of the site:

```html
<!doctype html>
<meta charset="utf-8">
<title>deadline-check overhead</title>
<pre id="out">measuring…</pre>
<script type="module">
  import init, { bench_clock } from "./pkg/spellchess_wasm.js";
  await init();
  bench_clock(200000); // warm up: first calls pay JIT tiering
  const ns = bench_clock(2000000);
  // Reference tree size for a depth-6 search from the starting position.
  // Replace with the real counts from the bench harness below.
  const nodes = 340000, qnodes = 2380000;
  const out = {
    ns_per_clock_read: Number(ns.toFixed(1)),
    reads_per_search: nodes + qnodes,
    seconds_of_overhead: Number((ns * (nodes + qnodes) / 1e9).toFixed(3)),
    percent_of_3s_budget: Number((ns * (nodes + qnodes) / 3e7).toFixed(1)),
  };
  document.getElementById("out").textContent = JSON.stringify(out, null, 2);
  console.log("clockbench:", JSON.stringify(out));
</script>
```

Get the real node counts first, then serve the page and read the result:

```bash
cd "/home/alex/Spell Chess"
SPELLCHESS_PROFILE=1 ~/.cargo/bin/cargo run --release -p spellchess-search --example bench 2>&1 | tail -20
python3 -m http.server -d web 8099
```

Open <http://localhost:8099/clockbench.html>. Substitute the harness's `nodes`
and `qnodes` into the page if they differ materially from the constants above.

**Reference measurements already taken on this machine** (Raspberry Pi,
aarch64, Chromium), using a standalone probe of the identical loop:

| Path | ns per clock read |
|---|---|
| Native `std::time::Instant` | **39** |
| wasm `web_time::Instant` via `performance.now()` | **138** |

So expect roughly **3–4× native**, about **0.38s per depth-6 search**, i.e.
**~12% of a 3s budget**. A result in that range is the expected outcome, not a
finding.

**Decision rule — read this before acting on the number:**

- **Under ~15% of the budget:** record it and move on. Nothing to do.
- **15–25%:** record it in the Task 10 report as a known cost. Still do nothing.
- **Over ~25%:** stop and escalate. The fix would be checking the clock every
  Nth node instead of every node, and that **changes the search hot loop** —
  it is gated on bit-identical node/qnode counts and belongs in its own change,
  reviewed on its own merits.

**Do not modify `timed_out()` or the search hot loop in this task under any
number.** This step measures; it does not optimise. Report the figure.

- [ ] **Step 6: Confirm native tests still pass**

```bash
cd "/home/alex/Spell Chess" && ~/.cargo/bin/cargo test --workspace 2>&1 | tail -15
```

Expected: PASS. The `bindings` module is `cfg(target_arch = "wasm32")`, so it compiles out natively and cannot break the workspace build.

- [ ] **Step 7: Commit**

```bash
cd "/home/alex/Spell Chess"
git add crates/wasm/src/lib.rs web/clockbench.html .gitignore Cargo.lock
git commit -m "feat(wasm): expose the engine to JavaScript as a Game class

A thin wasm-bindgen shell over the view layer: apply a turn, list legal
turns, undo, and search with a per-iteration progress callback. Errors
become JS exceptions rather than silently applying a different turn.

web/clockbench.html measures the per-node deadline check, which costs ~3.5x
more in wasm than natively because it crosses into performance.now()."
```

---

### Task 6: Worker, protocol, and a board that renders

The first end-to-end slice: the page loads the wasm in a worker and draws the starting position. No interaction yet.

**Files:**
- Create: `web/index.html`
- Create: `web/style.css`
- Create: `web/worker.js`
- Create: `web/app.js`

**Interfaces:**
- Consumes: the `Game` class from Task 5, imported from `./pkg/spellchess_wasm.js`.
- Produces: the worker message protocol, used unchanged by Tasks 7 and 8.
  - To worker: `{id, cmd: "state"}`, `{id, cmd: "turns"}`, `{id, cmd: "apply", from, to, promo, spell}`, `{id, cmd: "search", millis}`, `{id, cmd: "undo"}`, `{id, cmd: "reset"}`
  - From worker: `{id, ok: true, data}` | `{id, ok: false, error}` | `{type: "progress", depth, score, turn}`
  - In `app.js`: `async function call(cmd, args = {})` returning `data` or throwing.

- [ ] **Step 1: Write the worker**

Create `web/worker.js`:

```js
// Owns the game. The main thread holds no authoritative state, so every question
// about legality or position comes here. Search runs here too, which is the whole
// reason this is a worker: a search takes seconds and would freeze the page.
import init, { Game } from "./pkg/spellchess_wasm.js";

let game = null;

const ready = init().then(() => {
  game = new Game();
});

function handle(msg) {
  switch (msg.cmd) {
    case "state":
      return JSON.parse(game.state_json());
    case "turns":
      return JSON.parse(game.legal_turns_json());
    case "apply": {
      const spell = msg.spell ?? null;
      const json = game.apply_turn(
        msg.from,
        msg.to,
        msg.promo ?? undefined,
        spell ? spell.kind : undefined,
        spell ? spell.at : undefined,
      );
      return JSON.parse(json);
    }
    case "search": {
      const onProgress = (depth, score, turnJson) => {
        self.postMessage({ type: "progress", depth, score, turn: JSON.parse(turnJson) });
      };
      const result = game.search(msg.millis, onProgress);
      return JSON.parse(result);
    }
    case "undo":
      return game.undo();
    case "reset":
      game.reset();
      return JSON.parse(game.state_json());
    default:
      throw new Error(`unknown command: ${msg.cmd}`);
  }
}

self.onmessage = async (event) => {
  const msg = event.data;
  try {
    await ready;
    self.postMessage({ id: msg.id, ok: true, data: handle(msg) });
  } catch (err) {
    // A panic inside the engine poisons the instance, so report it rather than
    // going silent and leaving the board frozen with no explanation.
    self.postMessage({ id: msg.id, ok: false, error: String(err && err.message ? err.message : err) });
  }
};
```

- [ ] **Step 2: Write the page skeleton**

Create `web/index.html`. Every path is relative — the site is served from a repo subpath.

```html
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Spell Chess</title>
<link rel="stylesheet" href="./style.css">
</head>
<body>
<main>
  <h1>Spell Chess</h1>
  <p id="error" class="error" hidden></p>
  <div class="layout">
    <div>
      <div id="board" class="board"></div>
      <p id="status" class="status">Loading engine…</p>
    </div>
    <aside class="side">
      <section id="spells" class="spells"></section>
      <section class="controls">
        <label>Engine time
          <select id="millis">
            <option value="1000">1s</option>
            <option value="3000" selected>3s</option>
            <option value="10000">10s</option>
          </select>
        </label>
        <button id="new-game" type="button">New game</button>
        <button id="undo" type="button">Undo</button>
        <button id="cancel-spell" type="button" hidden>Cancel spell</button>
      </section>
      <section id="analysis" class="analysis"></section>
      <section><ol id="moves" class="moves"></ol></section>
    </aside>
  </div>
</main>
<script type="module" src="./app.js"></script>
</body>
</html>
```

- [ ] **Step 3: Write the styles**

Create `web/style.css`:

```css
:root { color-scheme: light dark; --light: #eadfc8; --dark: #9a7b52; --sel: #f2e06a; --tgt: #7fb069; --frozen: #6fa8dc; --jump: #c77dff; }
* { box-sizing: border-box; }
body { font-family: system-ui, sans-serif; margin: 0; padding: 1.5rem; }
main { max-width: 60rem; margin: 0 auto; }
h1 { font-size: 1.4rem; margin: 0 0 1rem; }
.layout { display: flex; gap: 1.5rem; flex-wrap: wrap; align-items: flex-start; }
.board { display: grid; grid-template-columns: repeat(8, 3rem); grid-template-rows: repeat(8, 3rem); border: 2px solid #444; }
.sq { display: flex; align-items: center; justify-content: center; font-size: 2rem; line-height: 1; cursor: pointer; user-select: none; position: relative; }
.sq.light { background: var(--light); } .sq.dark { background: var(--dark); }
.sq.selected { box-shadow: inset 0 0 0 4px var(--sel); }
.sq.target::after { content: ""; position: absolute; width: 1rem; height: 1rem; border-radius: 50%; background: var(--tgt); opacity: .8; }
.sq.spell-target { box-shadow: inset 0 0 0 4px var(--jump); }
.sq.frozen { background-image: linear-gradient(rgba(111,168,220,.55), rgba(111,168,220,.55)); }
.sq.jumped { background-image: linear-gradient(rgba(199,125,255,.55), rgba(199,125,255,.55)); }
.sq .piece.white { color: #fff; text-shadow: 0 0 2px #000, 0 0 2px #000; }
.sq .piece.black { color: #111; text-shadow: 0 0 2px #fff; }
.side { flex: 1 1 18rem; min-width: 16rem; }
.spells { margin-bottom: 1rem; }
.spell-row { display: flex; gap: .5rem; align-items: center; margin: .25rem 0; }
.spell-row button { min-width: 6rem; }
.spell-row button[disabled] { opacity: .45; }
.controls { display: flex; gap: .5rem; flex-wrap: wrap; align-items: center; margin-bottom: 1rem; }
.status { margin-top: .75rem; font-weight: 600; min-height: 1.4em; }
.analysis { font-variant-numeric: tabular-nums; min-height: 4rem; margin-bottom: 1rem; }
.analysis table { border-collapse: collapse; width: 100%; }
.analysis td { padding: .1rem .4rem; }
.moves { max-height: 14rem; overflow-y: auto; font-variant-numeric: tabular-nums; padding-left: 2.5rem; }
.error { color: #b00020; font-weight: 600; }
```

- [ ] **Step 4: Write the app shell that renders the board**

Create `web/app.js`:

```js
// The view. Holds no authoritative game state -- `state` below is only the last
// snapshot the worker sent, and every mutation goes back through the worker.
const worker = new Worker("./worker.js", { type: "module" });

let nextId = 1;
const pending = new Map();
let onProgress = () => {};

worker.onmessage = (event) => {
  const msg = event.data;
  if (msg.type === "progress") {
    onProgress(msg);
    return;
  }
  const entry = pending.get(msg.id);
  if (!entry) return;
  pending.delete(msg.id);
  if (msg.ok) entry.resolve(msg.data);
  else entry.reject(new Error(msg.error));
};

worker.onerror = (event) => {
  showError(`Engine failed to load: ${event.message ?? "unknown error"}`);
};

function call(cmd, args = {}) {
  const id = nextId++;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    worker.postMessage({ id, cmd, ...args });
  });
}

function showError(text) {
  const el = document.getElementById("error");
  el.textContent = text;
  el.hidden = false;
}

const GLYPHS = {
  wK: "♔", wQ: "♕", wR: "♖", wB: "♗", wN: "♘", wP: "♙",
  bK: "♚", bQ: "♛", bR: "♜", bB: "♝", bN: "♞", bP: "♟",
};

const FILES = "abcdefgh";
function squareName(index) {
  return FILES[index % 8] + (Math.floor(index / 8) + 1);
}

let state = null;

function renderBoard() {
  const board = document.getElementById("board");
  board.replaceChildren();
  const frozen = new Set();
  const jumped = new Set();
  for (const f of state.fields) {
    if (f.kind === "jump") jumped.add(f.square);
    else frozen.add(f.square);
  }
  // Rank 8 first so the DOM reads top-to-bottom the way the board looks.
  for (let rank = 7; rank >= 0; rank--) {
    for (let file = 0; file < 8; file++) {
      const index = rank * 8 + file;
      const name = squareName(index);
      const sq = document.createElement("div");
      sq.className = `sq ${(rank + file) % 2 === 0 ? "dark" : "light"}`;
      sq.dataset.square = name;
      if (frozen.has(name)) sq.classList.add("frozen");
      if (jumped.has(name)) sq.classList.add("jumped");
      const code = state.board[index];
      if (code) {
        const piece = document.createElement("span");
        piece.className = `piece ${code[0] === "w" ? "white" : "black"}`;
        piece.textContent = GLYPHS[code];
        sq.append(piece);
      }
      board.append(sq);
    }
  }
}

function renderStatus() {
  const el = document.getElementById("status");
  const s = state.status;
  if (s.kind === "checkmate") el.textContent = `Checkmate — ${s.winner} wins`;
  else if (s.kind === "kingCaptured") el.textContent = `King captured — ${s.winner} wins`;
  else if (s.kind === "stalemate") el.textContent = "Stalemate — draw";
  else el.textContent = `${state.sideToMove === "white" ? "White" : "Black"} to move`;
}

function render() {
  renderBoard();
  renderStatus();
}

async function boot() {
  try {
    state = await call("state");
    render();
  } catch (err) {
    showError(`Engine failed to start: ${err.message}`);
  }
}

boot();
```

- [ ] **Step 5: Serve and verify the board draws**

```bash
cd "/home/alex/Spell Chess/web" && python3 -m http.server 8099
```

Open `http://localhost:8099/` in a browser. Expected: an 8×8 board with the standard starting array in Unicode glyphs, and "White to move" underneath. The browser console must be free of errors.

If the module worker fails to load, confirm the server is sending `.js` as `text/javascript` — `python3 -m http.server` does. Do **not** work around it by switching the worker to a classic script; the wasm glue is an ES module.

- [ ] **Step 6: Commit**

```bash
cd "/home/alex/Spell Chess"
git add web/index.html web/style.css web/worker.js web/app.js
git commit -m "feat(web): render the board from a worker-hosted engine

The worker owns the Game and answers a small request/response protocol;
the page is a pure view. Establishes the message shapes the interaction
and analysis layers build on."
```

---

### Task 7: Playing a turn, including spells

**Files:**
- Modify: `web/app.js`

**Interfaces:**
- Consumes: `call()` and `render()` from Task 6; the `turns` / `apply` / `search` / `undo` / `reset` commands.
- Produces: `refresh()`, `engineMove()`, and the `staged` spell state that Task 8 reads.

- [ ] **Step 1: Add turn state and selection**

In `web/app.js`, after `let state = null;`, add:

```js
let legalTurns = [];
let selected = null;      // square name the user picked a piece on
let staged = null;        // {kind, at} spell staged for this turn, or null
let thinking = false;     // blocks input while the engine searches
```

- [ ] **Step 2: Filter turns by the staged cast**

Add these helpers. The filter is the crux of the whole UI: a spell can legalise a move that was illegal a moment earlier, so destinations must be computed **for the staged cast**, never independently of it.

```js
function sameSpell(a, b) {
  if (a === null && b === null) return true;
  if (a === null || b === null) return false;
  return a.kind === b.kind && a.at === b.at;
}

/// Turns available given whatever spell is currently staged.
function turnsForStaged() {
  return legalTurns.filter((t) => sameSpell(t.spell, staged));
}

function destinationsFrom(square) {
  return turnsForStaged().filter((t) => t.from === square).map((t) => t.to);
}

/// Squares this spell may legally target, taken from the legal turn list rather
/// than re-derived from the rules -- the engine already knows.
function spellTargets(kind) {
  const seen = new Set();
  for (const t of legalTurns) {
    if (t.spell && t.spell.kind === kind) seen.add(t.spell.at);
  }
  return seen;
}
```

- [ ] **Step 3: Extend the board render with highlights**

Replace the `renderBoard` function's inner square loop body so it also applies selection and target classes. Add these lines immediately before `board.append(sq);`:

```js
      if (selected === name) sq.classList.add("selected");
      if (selected && destinations.has(name)) sq.classList.add("target");
      if (armed && armedTargets.has(name)) sq.classList.add("spell-target");
      if (staged && staged.at === name) sq.classList.add("spell-target");
```

and add these lines at the top of `renderBoard`, just after `board.replaceChildren();`:

```js
  const destinations = new Set(selected ? destinationsFrom(selected) : []);
  const armedTargets = armed ? spellTargets(armed) : new Set();
```

Then declare `armed` alongside the other state at the top of the file:

```js
let armed = null;         // spell kind whose targets are being shown, or null
```

- [ ] **Step 4: Render the spell tray**

Add:

```js
function renderSpells() {
  const el = document.getElementById("spells");
  el.replaceChildren();
  for (const color of ["white", "black"]) {
    for (const kind of ["freeze", "jump"]) {
      const c = state.spells[color][kind];
      const row = document.createElement("div");
      row.className = "spell-row";
      const label = document.createElement("span");
      const cooldown = c.lock > 0 ? `cooldown ${c.lock}` : "ready";
      label.textContent = `${color} ${kind}: ${c.count} left, ${cooldown}`;
      if (color === state.sideToMove && color === "white") {
        const btn = document.createElement("button");
        btn.type = "button";
        btn.textContent = armed === kind ? "Pick target…" : `Cast ${kind}`;
        btn.disabled = thinking || staged !== null || spellTargets(kind).size === 0;
        btn.addEventListener("click", () => {
          armed = armed === kind ? null : kind;
          selected = null;
          render();
        });
        row.append(btn);
      }
      row.append(label);
      el.append(row);
    }
  }
  document.getElementById("cancel-spell").hidden = staged === null;
}
```

Update `render()` to call it:

```js
function render() {
  renderBoard();
  renderStatus();
  renderSpells();
  renderMoves();
}
```

- [ ] **Step 5: Handle clicks**

Add:

```js
function onSquareClick(name) {
  if (thinking || state.status.kind !== "inProgress") return;

  // Arming a spell: the click picks its target, and destinations are recomputed
  // for that cast -- moves illegal a moment ago may now be legal.
  if (armed) {
    if (spellTargets(armed).has(name)) {
      staged = { kind: armed, at: name };
      armed = null;
      selected = null;
      render();
    }
    return;
  }

  if (selected && destinationsFrom(selected).includes(name)) {
    playTurn(selected, name);
    return;
  }

  selected = destinationsFrom(name).length > 0 ? name : null;
  render();
}

document.getElementById("board").addEventListener("click", (event) => {
  const sq = event.target.closest(".sq");
  if (sq) onSquareClick(sq.dataset.square);
});
```

- [ ] **Step 6: Apply the turn and let the engine reply**

Add:

```js
const moveLog = [];

function renderMoves() {
  const el = document.getElementById("moves");
  el.replaceChildren();
  for (const text of moveLog) {
    const li = document.createElement("li");
    li.textContent = text;
    el.append(li);
  }
  el.scrollTop = el.scrollHeight;
}

/// Promotion is auto-queened. Underpromotion is legal and the engine generates
/// it, but a picker is not worth the UI surface here; a human who wants a knight
/// can say so in an issue.
function promoFor(from, to) {
  const match = turnsForStaged().find((t) => t.from === from && t.to === to && t.promo !== null);
  return match ? "q" : null;
}

async function refresh() {
  legalTurns = await call("turns");
  render();
}

async function playTurn(from, to) {
  const promo = promoFor(from, to);
  const chosen = turnsForStaged().find(
    (t) => t.from === from && t.to === to && t.promo === promo,
  );
  selected = null;
  const castThisTurn = staged;
  staged = null;
  try {
    state = await call("apply", { from, to, promo, spell: castThisTurn });
    moveLog.push(chosen ? chosen.text : `${from}${to}`);
    await refresh();
    if (state.status.kind === "inProgress") await engineMove();
  } catch (err) {
    showError(`Could not play that turn: ${err.message}`);
    await refresh();
  }
}

async function engineMove() {
  thinking = true;
  render();
  try {
    const millis = Number(document.getElementById("millis").value);
    const turn = await call("search", { millis });
    if (turn) {
      state = await call("apply", {
        from: turn.from,
        to: turn.to,
        promo: turn.promo,
        spell: turn.spell,
      });
      moveLog.push(turn.text);
    }
  } catch (err) {
    showError(`Engine error: ${err.message}`);
  } finally {
    thinking = false;
    await refresh();
  }
}
```

- [ ] **Step 7: Wire the buttons and update boot**

Add:

```js
document.getElementById("new-game").addEventListener("click", async () => {
  if (thinking) return;
  state = await call("reset");
  moveLog.length = 0;
  selected = null; staged = null; armed = null;
  await refresh();
});

// One undo steps back a single turn -- the engine's reply. A second steps back
// the user's own, which is what "take that back" usually means, so the button
// undoes twice when there is a full pair to remove.
document.getElementById("undo").addEventListener("click", async () => {
  if (thinking) return;
  for (let i = 0; i < 2; i++) {
    if (!(await call("undo"))) break;
    moveLog.pop();
  }
  state = await call("state");
  selected = null; staged = null; armed = null;
  await refresh();
});

document.getElementById("cancel-spell").addEventListener("click", () => {
  staged = null; armed = null; selected = null;
  render();
});
```

and change `boot()` to load the turn list too:

```js
async function boot() {
  try {
    state = await call("state");
    await refresh();
  } catch (err) {
    showError(`Engine failed to start: ${err.message}`);
  }
}
```

Then **move the bare `boot();` call to the very last line of `app.js`**. This task
added `const moveLog` and other top-level bindings below it, and `render()` reads
them; a `const` is in its temporal dead zone until the module reaches its
declaration. It happens to work today because `boot()` awaits before rendering, but
that is luck, not design. Confirm with:

```bash
cd "/home/alex/Spell Chess" && tail -1 web/app.js
```

Expected: `boot();`

- [ ] **Step 8: Play a real game in the browser**

```bash
cd "/home/alex/Spell Chess/web" && python3 -m http.server 8099
```

Walk this checklist at `http://localhost:8099/`, with the console open:

1. Click e2, then e4 — the pawn moves, the engine replies within the selected budget.
2. Click "Cast freeze" — candidate target squares outline in purple.
3. Pick a target, then make a move — the move list shows `freeze@<sq> <from><to>`, and a 3×3 block tints blue.
4. Confirm the frozen tint disappears after the field expires.
5. Cast jump and confirm the single square tints purple.
6. "Cancel spell" clears a staged cast and restores the unspelled destinations.
7. "Undo" steps back a full pair of turns.
8. "New game" resets board, spell counts, and move list.

Every step must be clean in the console. If highlights survive a turn they should not, the bug is a stale `selected`/`staged` — clear it in `playTurn`, not by re-rendering twice.

- [ ] **Step 9: Commit**

```bash
cd "/home/alex/Spell Chess"
git add web/app.js
git commit -m "feat(web): play turns, including spell casting

Destinations are always computed for the staged cast rather than
independently of it, since a spell can legalise a move that was illegal a
moment earlier. Spell targets and legality both come from the engine's
own legal turn list, so the UI cannot offer an illegal turn."
```

---

### Task 8: The live analysis panel

**Files:**
- Modify: `web/app.js`
- Modify: `web/index.html` (one button)

**Interfaces:**
- Consumes: the `{type: "progress", depth, score, turn}` messages from Task 6 and `onProgress` from Task 6.
- Produces: nothing later tasks depend on.

- [ ] **Step 1: Add the Analyze button**

In `web/index.html`, inside `<section class="controls">`, after the Undo button:

```html
        <button id="analyze" type="button">Analyze</button>
```

- [ ] **Step 2: Render analysis lines**

Add to `web/app.js`:

```js
// Depth, evaluation and best turn, updated per completed iteration. No principal
// variation: the engine tracks a best move, not a line, and reconstructing one
// means walking the transposition table.
let analysis = [];

/// Scores come back from the side to move's point of view. The panel reports
/// from White's, which is the convention every chess UI uses, so a reader is not
/// re-orienting the sign every ply.
function evalText(score, sideToMove) {
  const white = sideToMove === "white" ? score : -score;
  if (Math.abs(white) >= 99000) {
    const plies = 100000 - Math.abs(white);
    return `${white > 0 ? "+" : "-"}M${Math.ceil(plies / 2)}`;
  }
  return `${white >= 0 ? "+" : ""}${(white / 100).toFixed(2)}`;
}

function renderAnalysis() {
  const el = document.getElementById("analysis");
  el.replaceChildren();
  const heading = document.createElement("strong");
  heading.textContent = thinking ? "Thinking…" : "Analysis";
  el.append(heading);
  const table = document.createElement("table");
  for (const line of analysis.slice().reverse()) {
    const tr = document.createElement("tr");
    for (const cell of [`d${line.depth}`, line.eval, line.text]) {
      const td = document.createElement("td");
      td.textContent = cell;
      tr.append(td);
    }
    table.append(tr);
  }
  el.append(table);
}
```

Add `renderAnalysis();` to the end of `render()`.

- [ ] **Step 3: Feed the panel from the progress stream**

Replace the `let onProgress = () => {};` line's behaviour by assigning a real handler near the bottom of the file:

```js
onProgress = (msg) => {
  analysis.push({
    depth: msg.depth,
    eval: evalText(msg.score, state.sideToMove),
    text: msg.turn.text,
  });
  renderAnalysis();
};
```

- [ ] **Step 4: Clear the panel when a new search starts**

In `engineMove()`, immediately after `thinking = true;`, add:

```js
  analysis = [];
```

- [ ] **Step 5: Wire the Analyze button**

```js
// Analysing runs the same search on the user's own position without applying
// the result, so the panel works when it is your move.
document.getElementById("analyze").addEventListener("click", async () => {
  if (thinking) return;
  thinking = true;
  analysis = [];
  render();
  try {
    const millis = Number(document.getElementById("millis").value);
    await call("search", { millis });
  } catch (err) {
    showError(`Analysis failed: ${err.message}`);
  } finally {
    thinking = false;
    render();
  }
});
```

- [ ] **Step 6: Verify in the browser**

Serve `web/` again and confirm:

1. During the engine's think, rows appear one at a time as depths complete — not all at once at the end.
2. The deepest row's move matches the move the engine then plays.
3. "Analyze" on your own move fills the panel and changes nothing on the board.
4. A winning position for White shows a positive score; for Black, negative.
5. The 1s / 3s / 10s selector visibly changes how deep it gets.

- [ ] **Step 7: Commit**

```bash
cd "/home/alex/Spell Chess"
git add web/app.js web/index.html
git commit -m "feat(web): live analysis panel

Depth, evaluation and best turn stream in per completed iteration via the
search progress callback. Scores are normalised to White's point of view.
No principal variation -- the engine tracks a best move, not a line."
```

---

### Task 9: Build and deploy to GitHub Pages

**Files:**
- Create: `.github/workflows/pages.yml`
- Modify: `README.md`

**Interfaces:**
- Consumes: the `web/` directory and `crates/wasm` from earlier tasks.
- Produces: a deployed site at `https://alexcmarty.github.io/spell-chess-bot/`.

- [ ] **Step 1: Write the workflow**

Create `.github/workflows/pages.yml`:

```yaml
name: Deploy site to GitHub Pages

on:
  push:
    branches: [main]
  workflow_dispatch:

# Only the newest deploy matters; cancel superseded ones rather than queueing.
concurrency:
  group: pages
  cancel-in-progress: true

permissions:
  contents: read
  pages: write
  id-token: write

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Add the wasm target
        run: rustup target add wasm32-unknown-unknown

      - name: Cache cargo registry and build output
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            target
          key: wasm-${{ hashFiles('Cargo.lock') }}
          restore-keys: wasm-

      - name: Install wasm-pack
        # The prebuilt binary; `cargo install` would compile it from source on
        # every uncached run.
        run: |
          curl -sSL https://github.com/wasm-bindgen/wasm-pack/releases/download/v0.15.0/wasm-pack-v0.15.0-x86_64-unknown-linux-musl.tar.gz \
            | tar xz --strip-components=1 -C /usr/local/bin --wildcards '*/wasm-pack'
          wasm-pack --version

      - name: Build the engine to wasm
        run: wasm-pack build crates/wasm --target web --out-dir ../../web/pkg --release

      - uses: actions/configure-pages@v5
      - uses: actions/upload-pages-artifact@v3
        with:
          path: web

  deploy:
    needs: build
    runs-on: ubuntu-latest
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    steps:
      - id: deployment
        uses: actions/deploy-pages@v4
```

- [ ] **Step 2: Enable Pages with GitHub Actions as the source**

Pages is not currently enabled on this repository — `gh api repos/AlexCMarty/spell-chess-bot/pages` returns 404.

```bash
gh api -X POST repos/AlexCMarty/spell-chess-bot/pages \
  -f "build_type=workflow" 2>&1 | head -20
```

Expected: JSON describing the new Pages site. If it reports the site already exists, switch it instead:

```bash
gh api -X PUT repos/AlexCMarty/spell-chess-bot/pages -f "build_type=workflow"
```

- [ ] **Step 3: Document the site in the README**

Add to `README.md`, immediately after the opening description:

````markdown
## Play in the browser

The engine is compiled to WebAssembly and published at
**<https://alexcmarty.github.io/spell-chess-bot/>** — it runs entirely client-side,
with no server component. `.github/workflows/pages.yml` rebuilds and redeploys it on
every push to `main`.

To run the site locally:

```bash
wasm-pack build crates/wasm --target web --out-dir ../../web/pkg --release
python3 -m http.server -d web 8099
```

Then open <http://localhost:8099/>. `web/pkg/` is generated and gitignored.
````

- [ ] **Step 4: Commit and push**

```bash
cd "/home/alex/Spell Chess"
git add .github/workflows/pages.yml README.md
git commit -m "ci: build the wasm engine and deploy the site to GitHub Pages

Builds with a prebuilt wasm-pack on every push to main and publishes web/
via actions/deploy-pages. No build output is committed."
git push origin main
```

- [ ] **Step 5: Watch the run**

```bash
cd "/home/alex/Spell Chess"
gh run watch "$(gh run list --workflow=pages.yml --limit 1 --json databaseId -q '.[0].databaseId')" 2>&1 | tail -30
```

Expected: both jobs green. If the build job fails on `wasm-pack build`, reproduce locally with the identical command before changing the workflow — the failure is far more likely in the crate than in the YAML.

---

### Task 10: Verify the deployed site

A green workflow proves the artifact uploaded, not that the site works.

**Files:** none — verification only.

- [ ] **Step 1: Confirm the deployment URL**

```bash
gh api repos/AlexCMarty/spell-chess-bot/pages -q '.html_url, .status'
```

Expected: `https://alexcmarty.github.io/spell-chess-bot/` and `built`.

- [ ] **Step 2: Confirm the assets are actually being served**

```bash
for p in "" "app.js" "worker.js" "pkg/spellchess_wasm_bg.wasm"; do
  printf '%s -> ' "$p"
  curl -s -o /dev/null -w '%{http_code} %{content_type}\n' "https://alexcmarty.github.io/spell-chess-bot/$p"
done
```

Expected: `200` for all four, and `application/wasm` for the `.wasm`. A `404` on `pkg/` means the artifact was uploaded before the wasm build wrote into `web/pkg/` — check the step order in the workflow.

- [ ] **Step 3: Play a game on the live site**

Open the URL in a browser and repeat the Task 7 checklist plus the Task 8 checks, on the deployed copy. Confirm specifically:

- The board renders (proves relative paths survived the repo subpath).
- A freeze-enabled move and a jump-enabled move both work.
- The analysis panel streams during a think.
- The console is free of errors, including any mixed-content or CORS warnings.

- [ ] **Step 4: Report**

Report the live URL, the wasm binary size, and anything on the checklist that did not behave. Do not claim the site works without having played a turn on the deployed copy.

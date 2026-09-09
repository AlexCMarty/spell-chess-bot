# Design: Play Spell Chess in the browser (WASM on GitHub Pages)

**Date:** 2026-09-09
**Status:** Approved, not yet implemented

## Goal

A static site on GitHub Pages where a visitor plays Spell Chess against this engine.
The engine runs client-side as WebAssembly. There is no server component and no
network call after the page loads.

Scope: play against the engine, plus a live analysis panel. Not in scope: local
two-player hotseat, opening books, accounts, saved games, or any multiplayer.

## Constraints discovered in the existing code

These are facts about the current tree, verified before the design was written.
They drive most of the decisions below.

1. **`search_smp` calls `Instant::now()` unconditionally** (`search.rs:680`), before
   it branches on `Budget`. On `wasm32-unknown-unknown`, `std::time::Instant::now()`
   panics. So a clock fix is required even for a pure depth-limited search.
2. **Multithreading is unavailable in the browser.** `default_threads()` already
   returns 1 and Lazy-SMP is a measured net loss on this engine, so nothing is lost.
3. **`std::thread::scope` with zero spawns is fine on wasm.** Probed directly: it
   compiles and returns the correct value under Node. `search_smp(pos, budget, 1)`
   never enters the `(1..1)` spawn loop, so no cfg bypass is needed.
4. **There is no position serializer.** The `fen` REPL command is `format!("{:?}")`
   with no matching parser. Nothing can round-trip a `Position` through a string.
5. **`cli/src/notation.rs` imports only `spellchess_core`.** It has no CLI-specific
   dependency and can move without disturbing anything.
6. **A search allocates ~84MB of transposition table per call** (`TT_SIZE = 1 << 21`,
   built fresh inside `search_smp`).

## Architecture

### The WASM boundary

One WASM instance, running inside a **Web Worker**, owns the game. The main thread is
a pure view: it sends commands and renders the JSON that comes back. It holds no
authoritative game state of its own.

This follows from constraint 4. The alternative — a second instance on the main
thread answering legality synchronously — requires shipping a `Position` between two
instances, which means building a serializer and parser that do not exist, and
introduces two copies of the game state that can disagree. The cost of the chosen
design is a sub-millisecond `postMessage` round-trip to ask "what are my legal
moves". That is cheaper than a class of desync bugs.

Running the search on the main thread is not an option: a search takes seconds and
would freeze the page. Making it yield periodically would mean threading yield points
through `negamax`, which risks the node-count invariant.

```
main thread                      Web Worker
-----------                      ----------
app.js  ── command ─────────────▶ worker.js
        ◀──── state JSON ─────── spellchess_wasm::Game
                                   └── Position + history (authoritative)
```

### Crate layout

A fourth workspace member, `crates/wasm` (`spellchess-wasm`), `crate-type = ["cdylib",
"rlib"]`, depending on `core` and `search`.

It exposes a single `#[wasm_bindgen] pub struct Game` holding a `Position` and an undo
history, with methods returning JSON strings:

| Method | Returns |
|---|---|
| `new()` / `reset()` | — |
| `undo()` | bool (false if history empty) |
| `state_json()` | board, side to move, per-side spell counts and cooldowns, live freeze/jump fields, game status |
| `legal_turns_json()` | every legal turn as `{from, to, promo, spell}` |
| `apply_turn(from, to, promo, spell)` | Result: new state, or an error string |
| `search(millis, on_progress)` | best turn + score; `on_progress` is a `js_sys::Function` invoked with `(depth, score, turn_json)` per completed depth |

Squares cross the boundary as algebraic strings (`"e4"`). `spell` is `null` for no
cast, or `{kind: "freeze"|"jump", at: "e4"}`. `promo` is `null` or one of
`"q"|"r"|"b"|"n"`. These are the only shapes the boundary accepts; the JSON layer
rejects anything else with an error string rather than guessing.

**Testability rule:** every JSON-building function is a plain Rust `fn` taking and
returning ordinary types, with no `wasm_bindgen` types in its signature. The
`#[wasm_bindgen]` methods are thin shells over them. This keeps the entire JSON layer
under `cargo test -p spellchess-wasm` on the native target, where it can be tested
without a browser.

**Legality comes from `generate_turns`** — the exhaustive slow oracle, not one of the
search generators. It runs once per user turn, where its cost is irrelevant, and being
the reference generator means the UI cannot offer an illegal turn.

### Changes to existing crates

**`crates/search` — replace `std::time::Instant` with `web_time::Instant`.**
Added as `[target.'cfg(target_arch = "wasm32")'.dependencies]`. Off wasm, `web-time`
re-exports `std::time` verbatim, so native behaviour is bit-identical and `Duration`
signatures are unchanged. The alternative, a hand-written cfg-gated shim over
`js_sys::Date::now()`, carries the same wasm-only dependency cost with ~40 more lines
to own. This is the one change that alters a property of the project: all three crates
currently have zero non-dev dependencies. The dependency is invisible to native
builds.

**`crates/search` — add `search_with_progress`.**

```rust
pub fn search_with_progress(
    pos: &Position,
    budget: Budget,
    threads: usize,
    on_iteration: &mut dyn FnMut(u32, i32, &Turn),
) -> Option<(Turn, i32)>
```

`search_smp` becomes this call with a no-op callback, keeping its signature and every
existing caller untouched. The callback fires once per completed iterative-deepening
iteration and feeds the analysis panel.

This is an API addition inside the deepening loop, not a change to the search tree.
**Node and qnode counts must remain bit-identical**, verified with `bench` before and
after, per the standing invariant.

**`crates/core` — receive `notation.rs` from `crates/cli`.**
`cli/src/notation.rs` moves to `core/src/notation.rs`; the old path becomes a
re-export so the CLI and its tests are unchanged. `Turn ↔ string` is shared
vocabulary rather than CLI presentation, and this stops the wasm crate from having to
depend on the CLI crate to format a move. (`render.rs` is genuinely CLI-specific and
stays put.)

### The site

`web/`, with no bundler and no npm:

```
web/
  index.html
  style.css
  app.js       view + input handling
  worker.js    module worker; owns the Game
  pkg/         generated by wasm-pack, gitignored
```

`wasm-pack build --target web` output, loaded as ES modules; the worker is
`type: "module"`.

**Interaction model.** Click a piece to highlight its legal destinations. A spell tray
shows each spell's remaining count and cooldown for both sides. Arming a spell
highlights its legal targets; staging a cast **recomputes the move highlights for
turns carrying that cast**, which is the central UI requirement — a spell can legalise
a move that was not legal a moment earlier, and a UI that filtered moves independently
of the staged cast would be wrong. Cancelling the cast restores the unspelled set.

Board state always shows: frozen squares tinted, the live jump square marked, both
sides' spell counts and cooldowns, and a status banner for check, stalemate, or king
captured. A move list renders turns via `format_turn`.

Pieces are Unicode glyphs, styled with CSS. Zero assets and no licensing question;
swappable for an SVG set later.

**Analysis panel.** Depth, evaluation in pawns from White's point of view, and the
current best turn, updating live per completed iteration while the engine thinks, plus
an "Analyze" button for the position on the user's own move.

No principal variation is shown. The engine tracks a best move, not a PV;
reconstructing a line means walking the transposition table, which is out of scope.

**Strength control.** A time-budget slider (`Budget::Time`), not fixed depth, so the
engine adapts to the visitor's hardware instead of stalling a slow phone.

### Deployment

`.github/workflows/pages.yml`: checkout, add the `wasm32-unknown-unknown` target,
fetch the prebuilt `wasm-pack` binary, build release, upload `web/` as a Pages
artifact, deploy with `actions/deploy-pages`. No build output is committed.

Pages is not currently enabled on the repository (`GET /repos/.../pages` returns 404).
It will be enabled with `build_type: workflow` via `gh api`.

The site is served from a repository subpath
(`https://alexcmarty.github.io/spell-chess-bot/`), so every asset path must be
relative. No absolute-rooted URLs.

## Error handling

- **Illegal turn submitted.** `apply_turn` returns an error string; the UI clears the
  staged turn and re-renders from `state_json()`. The UI should never reach this, since
  it only offers turns from `legal_turns_json()`, so it is treated as a bug signal
  rather than an expected path.
- **Worker fails to load or the WASM fails to instantiate.** The page shows a plain
  error state instead of a dead board.
- **Search returns `None`** (no legal turn). The game is over; the status banner from
  `game_status` explains why.
- **Panic inside WASM.** A panic poisons the instance. `console_error_panic_hook` is
  installed so the cause is legible, and the worker reports the failure to the main
  thread rather than going silent.

## Testing

| Layer | How |
|---|---|
| JSON view functions | `cargo test -p spellchess-wasm`, native target |
| `search_with_progress` | native test that the callback fires once per depth and the final result equals `search_smp`'s |
| Node-count invariant | `bench` before and after the search change; counts must be identical |
| Existing suites | `cargo test --workspace` stays green through the `notation.rs` move |
| End to end | `python3 -m http.server` over `web/`, then play a real game in a browser: a plain move, a freeze-enabled move, a jump-enabled move, undo, and an engine reply |

The browser pass happens before anything is deployed.

## Open risk

**Memory.** `TranspositionTable::new()` allocates ~84MB per search call, so wasm linear
memory settles near ~90MB and never shrinks. This is kept identical to native so that
the browser and the desktop binary play at the same strength. If mobile browsers prove
to OOM, the fix is a wasm-only smaller `TT_SIZE` — which would make wasm results
diverge from native, and so is not done pre-emptively.

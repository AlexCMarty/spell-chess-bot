---
name: core-api-changes
description: Use when changing spellchess-core's public API — anything re-exported from crates/core/src/lib.rs, including Position, Turn, SpellCast, Delta, captures_enabled_by, legal_moves, generate_turns and its five siblings, spells::jump_targets/freeze_targets, or any type/field the JSON layer or fuzz targets touch. Fires whenever cargo build/test --workspace passes and you're about to call the change done: --workspace does not compile crates/wasm's wasm32 half, does not run anything under web/, and does not touch crates/core/fuzz (its own [workspace] table). Gives the checklist and verification command for each of the three blind spots plus the one thing --workspace does catch.
---

# Changing spellchess-core's public API

`cargo build --workspace` and `cargo test --workspace` are not the finish line for a
`spellchess-core` API change. Three of the four things that can break sit entirely outside
what either command compiles. This is the checklist and the command for each.

## What counts as "the public API"

Everything re-exported by `crates/core/src/lib.rs` — the `pub use` lines, not just `pub
mod`. As of writing that's `types::*`, `Bitboard`, `Board`, `FieldSet`, `position::*`,
`{PieceMove, Promotion, pseudo_legal_moves}`, `{parse_turn, format_turn}`,
`{attacker_count, attackers_to, is_square_attacked}`, `{captures_enabled_by, Delta}`, and
from `legal`: `Turn`, `SpellCast`, all six `generate_*` turn generators, `legal_moves`,
`legal_captures`, `apply_move_only`, `apply_turn`. `spells` and `terminal` are exported as
modules (`spells::jump_targets`, `game_status`, etc.), so anything public inside them
counts too, even though the module itself isn't in that flat list.

A concrete breaking-change test: if you rename, remove, or change the signature/field set
of anything in that list, or change what a `Delta` variant or a `GameStatus`/`SpellKind`
variant means, grep the three consumer surfaces below for its name before you consider the
change safe. A change that's purely additive (a new variant nobody matches on yet, a new
function) is exactly the shape that breaks `web/app.js`'s `status.kind` switch silently —
see below — so "additive" is not automatically "safe" for that one consumer.

## The checklist

Copy this as a todo list for any PR that touches `spellchess-core`'s public surface:

1. `cargo test --workspace` — the baseline. Catches nothing in items 2–4 below by itself.
2. `cargo test -p spellchess-wasm` — **this one `--workspace` already covers**, but is
   worth calling out on its own because it's the one native, compiled check on the JSON
   layer. `crates/wasm/src/view.rs` has no `wasm_bindgen` types in any signature (that's
   the whole point of the split — see `docs/WEB.md`'s Testability rule), so it and its
   `mod tests` compile and run on the native target via `spellchess-wasm`'s `rlib` crate
   type. If a rename in `core` breaks `view.rs`, this fails before anything reaches wasm.
3. `wasm-pack build crates/wasm --target web` — type-checks the
   `#[cfg(target_arch = "wasm32")]` half of `crates/wasm/src/lib.rs` (the `bindings`
   module: `Game`, `apply_turn`, `search`, `bench_clock`). No native build ever compiles
   this half. One-time prerequisite: `rustup target add wasm32-unknown-unknown`.
   `.github/workflows/pages.yml` runs this exact build on every PR as its de facto
   type-check — but that's a CI round-trip; run it locally before pushing; it needs `wasm-pack` installed (CI pins the v0.15.0 prebuilt binary; `cargo install wasm-pack` also works but compiles from source).
4. Read `docs/WEB.md`'s "The state JSON" and "The worker command protocol" sections and
   manually re-check `web/app.js` against your change. There is no compiler and no test
   for this side — see the next section for exactly what to look at.
5. `cargo +nightly fuzz build` from `crates/core/fuzz/` — type-checks (not runs)
   `fuzz_freeze` and `fuzz_jump` against the new API. Neither prerequisite is installed by
   default: `rustup toolchain install nightly` and `cargo install cargo-fuzz` first.

```sh
cd crates/core && cargo +nightly fuzz build
```

Do steps 2 and 3 for *any* change to the exported surface; do step 4 only if the change
touches something `docs/WEB.md` documents (below); do step 5 only if the change touches
something the fuzz targets actually call (also below) — check before assuming either is
irrelevant, since both lists are easy to get wrong by memory.

## Step 4 in detail: the runtime-only JSON contract

`web/app.js` has no compile-time relationship to `spellchess-core` at all — it consumes a
JSON string produced by `crates/wasm/src/view.rs`, across a Web Worker boundary, and
nothing in `web/` has any test coverage. `docs/WEB.md` is the canonical description of
that contract; don't restate it here, but specifically re-check these shapes, because each
one breaks *silently* (no exception, no console error, just wrong-looking UI) if `core`
changes underneath it:

- **`board`** is indexed `rank * 8 + file`, and an empty square is `""`, not `null`.
  `app.js` branches on that empty string; a `core`-side change that makes an empty square
  serialize differently (or a piece-code change in `view.rs`'s `piece_code`) silently
  breaks rendering with no error.
- **`status.kind`** is a closed four-string switch in `app.js`
  (`inProgress | stalemate | checkmate | kingCaptured`). If `core`'s `GameStatus` grows a
  fifth variant and `view.rs`'s `status_json` starts emitting a fifth string, `app.js`
  falls through and renders a finished game as still in progress — no crash, just a stale
  board. Grep `app.js` for the switch before adding a `GameStatus` variant.
- **`undo`**'s reply is a **bare boolean**, not wrapped in an object, deliberately
  asymmetric with every other command. Don't "fix" that consistency in `view.rs` or
  `lib.rs` without updating `app.js`'s undo loop, which tests the value directly as a bool.
- **`spell`**/**`promo`** boundary shapes are exactly `null | {kind: "freeze"|"jump", at:
  "e4"}` and `null | "q"|"r"|"b"|"n"` — `view.rs`'s `parse_spell`/`parse_promo` reject
  anything else with an error string. A new `SpellKind` or `Promotion` variant in `core`
  needs a matching string on both the `view.rs` encode side and whatever `app.js` sends.

If your change touches `SpellState`/`SpellCounter` (the `count`/`lock` shape), field
lifetime (`expiresAfterPly`), or adds a new terminal status, treat `docs/WEB.md` as
required reading before you touch `view.rs`, not after.

## Step 5 in detail: what the fuzz targets actually depend on

`crates/core/fuzz/fuzz_targets/fuzz_freeze.rs` and `fuzz_jump.rs` are near-identical and
both call, directly: `spells::freeze_targets` / `spells::jump_targets` (to pick which
casts to try), `captures_enabled_by` (the fast path under test) with its `Delta` return
type, and `legal_moves` (both for the pre-cast baseline and inside each target's own
`rescan_oracle` used as the differential ground truth). They also construct `Position`,
`SpellField`, `SpellCast`, `PieceMove`, `Board`, `Square`, `Piece`, `Color`, `PieceKind`,
and `CastleRights` directly, so a field rename on any of those needs the fuzz targets
updated too, or `cargo +nightly fuzz build` won't compile. This list matches what
CLAUDE.md and the `spell-legality-testing` skill name (`captures_enabled_by`, `Delta`,
`spells::jump_targets`/`freeze_targets`) — verified directly against both target files
rather than assumed, so treat it as current.

Because `crates/core/fuzz/` carries its own `[workspace]` table
(`crates/core/fuzz/Cargo.toml`), it is invisible to `cargo build|test --workspace` from the
repo root — not slow to include, structurally excluded.

## Hand-offs

- Touching freeze/jump legality itself (not just its surrounding types) — the
  `spell-legality-testing` skill governs the testing discipline for that, including why a
  green fuzz/soundness run doesn't prove correctness here.
- A change that also alters search cost or node counts — the `perf-measurement` skill
  governs, and the node-count invariant in `docs/ARCHITECTURE.md#invariants` applies
  regardless of whether the API change was "just" a rename.

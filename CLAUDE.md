This is a project to create a bot that can beat humans at the game of Spell Chess at `https://www.chess.com/variants/spell-chess/analysis`!

**Only ever open `https://www.chess.com/variants/spell-chess/analysis`.** Other pages can
match you into a game against a human, which would be a terms-of-service violation. A
`PreToolUse` hook (`.claude/hooks/guard-chesscom-url.py`) backs this up, but it is a speed
bump, not a wall: it only inspects `Bash` commands that look like a fetch and URL-shaped
fields on `WebFetch`/browser-automation tools, doesn't cover `WebSearch`, doesn't follow
redirects, and exits 0 (allow) on its own internal errors by design. Passing it is not proof
you're in the clear — the rule above is what you must actually follow. If the hook does block
you, it is right and you must not work around it.
([Issue #10](https://github.com/AlexCMarty/spell-chess-bot/issues/10) tracks tightening it.)

## Where knowledge lives

- [`rules/INDEX.md`](rules/INDEX.md) — the rules. Read before writing move-generation code.
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — how the Rust fits together, and which
  two files hold most of the difficulty.
- [`docs/WEB.md`](docs/WEB.md) — the browser build: `crates/wasm`, the worker protocol,
  the JSON contract with `web/`, and the wasm-only traps.
- `/perf-measurement` skill — before benchmarking or optimizing anything.
- `/spell-legality-testing` skill — before touching freeze/jump legality or its tests.
- `/oracle-verification` skill — before opening any chess.com URL, adding a fixture, or
  re-pinning the engine bundle.
- `/core-api-changes` skill — before calling a `spellchess-core` public API change done.
- [GitHub issues](https://github.com/AlexCMarty/spell-chess-bot/issues) — the open backlog,
  including work deliberately paused.

## Commands

```sh
cargo build --workspace                                        # not wasm32 or fuzz — see Workspace
cargo test --workspace                                         # same carve-outs
cargo run --release -p spellchess-cli                          # the REPL
cargo run --release -p spellchess-search --example bench -- 6  # the perf number that counts
cargo test -p spellchess-core --release -- --ignored           # #[ignore]d batteries `cargo test` skips
wasm-pack build crates/wasm --target web --out-dir ../../web/pkg --release
```

## Workspace

Four-crate Cargo workspace: `crates/core` (board, movegen, rules engine — no internal deps),
`crates/search` (search algorithms, depends on `core`), `crates/cli` (the `spellchess` binary,
depends on both), and `crates/wasm` (the `wasm-bindgen` boundary for the browser front end in
`web/`, depends on both). Scope to `-p spellchess-core` etc. to iterate on one crate.

`cargo build --workspace` / `cargo test --workspace` do **not** cover everything. Two carve-outs:

1. The `#[cfg(target_arch = "wasm32")]` half of `crates/wasm/src/lib.rs`. A native build never
   compiles it; only `wasm-pack build crates/wasm --target web` does, which CI runs on every PR
   via `.github/workflows/pages.yml`. Nothing in `web/` is covered at all.
2. `crates/core/fuzz/`, below.

`.github/workflows/ci.yml` runs `cargo clippy --workspace --all-targets` and
`cargo test --workspace` on every PR, plus a separate job running `cargo +nightly fuzz build`.
It does **not** run the `#[ignore]`d batteries above or anything in `web/` — those, like the
wasm-pack type-check above, stay yours to run locally.

**If you change `spellchess-core`'s public API, four things can break and only one of them is
in `--workspace`:** `crates/wasm/src/view.rs`, the JSON shape `web/app.js` reads (see
[`docs/WEB.md`](docs/WEB.md)), the wasm32-only half of `lib.rs`, and the fuzz targets.
The `/core-api-changes` skill carries the checklist and the verification command for each.

`crates/core/fuzz/` is a `cargo-fuzz` crate (targets `fuzz_freeze` and `fuzz_jump`) with its own
`[workspace]` table, deliberately isolated from the root workspace — so it is **not** covered by
`cargo build --workspace` / `cargo test --workspace`. Run it with, e.g.,
`cd crates/core && cargo +nightly fuzz run fuzz_jump -- -max_total_time=120` (requires a nightly
toolchain). Run `cargo +nightly fuzz build` manually after any change to `spellchess-core`'s
public API (`captures_enabled_by`, `Delta`, `spells::jump_targets`/`freeze_targets`, etc.) that
the fuzz targets depend on, since nothing else will catch a break there.

## Ruleset

Full rule corpus lives in [`rules/`](rules/), routed from [`rules/INDEX.md`](rules/INDEX.md) —
read that first; it points to the exact file/anchor for any question. Rules are reverse-engineered
from chess.com's own client engine (`variants.js`) and verified by executing that engine, since
public chess.com documentation of this variant is thin and wrong in several places. Each rule
statement is tagged `[VERIFIED]`, `[CODE]`, `[DOC]`, or `[UNVERIFIED]` — when code and prose
disagree, code wins.

`research/variants.js` / `research/variants.pretty.js` — that engine — are third-party
copyrighted code: gitignored, kept locally only, never committed or redistributed.

<!-- Canonical: rules/20-spell-system.md and rules/INDEX.md. This is a deliberate front-door
     summary; keep it in sync with them rather than treating it as its own source of truth. -->

### The short version

- Each turn is **one optional spell + one mandatory piece move**. You can never pass.
- **Freeze** (5 per game, 3-turn cooldown): immobilises a 3×3 block, including the caster's own
  pieces. Frozen pieces exert **no control at all** — no check, no guarded squares — but still
  occupy their square and can still be captured.
- **Jump** (2 per game, 3-turn cooldown): makes one occupied square transparent to sliders, for
  **both players**. Landing on a live jump square does not block a check through it; other
  squares on the ray still can.
- **The king can be captured, and it wins.** A spell cast at the start of your turn can create an
  attack the opponent never had a chance to answer.
- **Checkmate is not checkmate** while the defender still holds an unlocked spell that could
  produce a legal move.
- Spell counts never replenish; only the cooldown resets.

## Hard rules

The bullets below are the front-door summary; the full statement, and the rest of the search
invariants, is canonical in
[`docs/ARCHITECTURE.md#invariants`](docs/ARCHITECTURE.md#invariants).

<!-- Canonical: docs/ARCHITECTURE.md#invariants. Update these bullets when that section changes. -->

- **A performance change must leave node/qnode counts bit-identical.** If they move, you
  changed the search tree, not the cost per node — that needs correctness review, not a
  benchmark. Speed work must not narrow the tree.
- **Never delete the slow rescan path.** It is the differential oracle the fast paths in
  `spell_delta.rs` are tested against. A fast path that cannot model a geometry returns
  `Delta::NeedsRescan`; it does not guess.
- **Use `--no-fail-fast` for mutation testing.** Plain `cargo test` stops at the first
  failing target and will mis-attribute kills.
- `cargo` may be at `~/.cargo/bin/cargo` and not on `PATH`. Release builds take a couple of
  minutes; a depth-8 benchmark once took ~90–150s **on a Raspberry Pi 5** — cited only so a
  slow build isn't mistaken for a hang, not as an expectation for your hardware. Establish
  your own baseline and work in ratios: see the `/perf-measurement` skill.

## Commits

You MUST use [Conventional Commits](https://www.conventionalcommits.org/) for every commit
message (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, etc.).
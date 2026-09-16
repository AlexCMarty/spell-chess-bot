# Spell Chess Bot

Goal: a bot that beats humans at [Spell Chess](https://www.chess.com/variants/spell-chess),
chess.com's chess variant with castable spells.

**If you're driving this repo's tooling (or an AI agent) against chess.com, only ever open
`https://www.chess.com/variants/spell-chess/analysis`.** Other pages can match an automated
client into a live game against a human, which would be a terms-of-service violation.

## Play in the browser

The engine is compiled to WebAssembly and published at
**<https://alexcmarty.github.io/spell-chess-bot/>** — it runs entirely client-side,
with no server component. `.github/workflows/pages.yml` rebuilds and redeploys it on
every push to `main`.

To run the site locally you need the wasm target and `wasm-pack`, neither of which comes
with a default toolchain:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack          # CI pins v0.15.0 from a prebuilt tarball

wasm-pack build crates/wasm --target web --out-dir ../../web/pkg --release
python3 -m http.server -d web 8099
```

Then open <http://localhost:8099/>. `web/pkg/` is generated and gitignored.

The same workflow also builds to wasm on **every pull request**, purely as a type-check:
the `#[cfg(target_arch = "wasm32")]` half of `crates/wasm/src/lib.rs` is invisible to
`cargo test`. The deploy uploads *all* of `web/`, so anything left there is published.
[`docs/WEB.md`](docs/WEB.md) covers the architecture and the wasm-only traps.

## What's here

A Rust workspace with a rules-accurate move generator, an alpha-beta search, a REPL to
drive both, and a WebAssembly build behind the site:

| Crate | Covers |
|---|---|
| [`crates/core`](crates/core) | Board representation, legal move/spell generation, the rules engine. No internal deps. |
| [`crates/search`](crates/search) | Alpha-beta search with quiescence, built on `core`. Lazy-SMP is implemented but defaults **off** — it measured a net loss here. |
| [`crates/cli`](crates/cli) | The `spellchess` binary: a REPL for playing out positions and asking the engine for a move. |
| [`crates/wasm`](crates/wasm) | The `wasm-bindgen` boundary: `lib.rs` is the browser shell, `view.rs` the natively-testable JSON layer. |
| [`web/`](web) | The static front end — `app.js` (view), `worker.js` (owns the game), `primer.js` (rules cards). See [`docs/WEB.md`](docs/WEB.md). |

[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) explains how those fit together, which two
files hold most of the difficulty, and the invariants a change has to preserve. Read it
before making a non-trivial change. [`docs/WEB.md`](docs/WEB.md) does the same for the
browser build.

Commits use [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`,
`docs:`, …).

## Build and run

```sh
cargo build --workspace
cargo test --workspace
cargo run --release -p spellchess-cli
```

The REPL takes one command per line:

```
> newgame
> board                       # render the current position
> e2e4                        # play a move
> freeze@d5 g8f6              # cast a spell, then move
> go --depth 8                # ask the engine for its best turn
> go --time 5                 # or budget by seconds
> undo
```

For raw search performance, use the `bench` example rather than the CLI binary — it isolates
the search from the REPL and avoids comparing against a stale build (see the doc comment in
[`crates/search/examples/bench.rs`](crates/search/examples/bench.rs) for why that distinction
matters):

```sh
cargo run --release -p spellchess-search --example bench -- 8
```

## Start here for the rules

**[`rules/INDEX.md`](rules/INDEX.md)** — the rule corpus. It is a routing index; read it
first and it will send you to the one file that answers your question.

The rules were reverse-engineered from chess.com's own client engine
(`variants/2026.8.1/variants.js`) and then **verified by executing that engine** against
purpose-built positions. Public documentation of this variant is thin and in several places
wrong; the corpus flags each rule as `[VERIFIED]`, `[CODE]`, `[DOC]` or `[UNVERIFIED]` so
you know what is load-bearing.

## The corpus

| File | Covers |
|---|---|
| [`rules/INDEX.md`](rules/INDEX.md) | Routing table + the 12 rules that surprise implementers |
| [`rules/00-overview.md`](rules/00-overview.md) | Canonical config, coordinate model, errors in public docs |
| [`rules/10-base-chess.md`](rules/10-base-chess.md) | Which orthodox rules apply |
| [`rules/20-spell-system.md`](rules/20-spell-system.md) | Turn structure, counts, cooldowns, field lifetime |
| [`rules/30-freeze.md`](rules/30-freeze.md) | The freeze spell, complete |
| [`rules/40-jump.md`](rules/40-jump.md) | The jump spell, complete |
| [`rules/50-interactions.md`](rules/50-interactions.md) | Win conditions, mate detection, castling, en passant |
| [`rules/60-notation-and-encoding.md`](rules/60-notation-and-encoding.md) | SAN / PGN / FEN formats |
| [`rules/70-engine-api.md`](rules/70-engine-api.md) | Driving the real engine as a test oracle |
| [`rules/90-test-vectors.md`](rules/90-test-vectors.md) | 20 positions with verified expected output |

## The short version

- Each turn is **one optional spell + one mandatory piece move**. You can never pass.
- **Freeze** (5 per game, 3-turn cooldown): immobilises a 3×3 block. Frozen pieces exert
  **no control at all** — no check, no guarded squares.
- **Jump** (2 per game, 3-turn cooldown): makes one occupied square transparent to sliders,
  **for both players**.
- **The king can be captured, and it wins.** A spell cast at the start of your turn can
  create an attack the opponent never had a chance to answer.
- **Checkmate is not checkmate** while the defender still holds an unlocked spell that
  could produce a legal move.

## Working on this repo

[`rules/70-engine-api.md`](rules/70-engine-api.md) has a copy-pasteable harness for running
chess.com's engine in the browser. Use it to validate any move generator you write.

Two things are worth knowing before you optimize or touch spell legality, because both
have burned this project repeatedly:

- **A performance change must leave node/qnode counts bit-identical.** If they move, you
  changed the search tree rather than the cost per node.
- **Random fuzzing does not find legality bugs here.** Every one this project has shipped
  was caught by a hand-built adversarial position or by mutation-testing a guard. The
  generated batteries are a regression net.

If you use Claude Code, both are written up as skills in `.claude/skills/` and load
automatically when relevant. The repo also ships a `PreToolUse` hook that blocks any
chess.com URL other than the analysis board — you will be asked to trust it on first run.
Verify it with `bash .claude/hooks/test-guard-chesscom-url.sh`.

### The engine bundle

`research/variants.js` is chess.com's own client engine. It is third-party copyrighted
code, so it is **gitignored and never redistributed** — you fetch your own copy:

```sh
mkdir -p research && cd research
curl -O https://www.chess.com/r2/client-packages/variants/2026.8.1/variants.js
```

The version string moves; check the analysis page's network tab if that path 404s. Nothing
in `cargo test --workspace` needs this file — it is only for re-deriving or re-verifying
rules against the real engine.

## License

[MIT](LICENSE)

# Spell Chess Bot

Goal: a bot that beats humans at [Spell Chess](https://www.chess.com/variants/spell-chess),
chess.com's chess variant with castable spells.

**If you're driving this repo's tooling (or an AI agent) against chess.com, only ever open
`https://www.chess.com/variants/spell-chess/analysis`.** Other pages can match an automated
client into a live game against a human, which would be a terms-of-service violation.

## What's here

A Rust workspace with a rules-accurate move generator, a parallel alpha-beta search, and a
REPL to drive both:

| Crate | Covers |
|---|---|
| [`crates/core`](crates/core) | Board representation, legal move/spell generation, the rules engine. No internal deps. |
| [`crates/search`](crates/search) | Alpha-beta search with quiescence and Lazy-SMP, built on `core`. |
| [`crates/cli`](crates/cli) | The `spellchess` binary: a REPL for playing out positions and asking the engine for a move. |

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

`rules/70-engine-api.md` has a copy-pasteable harness for running chess.com's engine in the
browser. Use it to validate any move generator you write.

## License

[MIT](LICENSE)

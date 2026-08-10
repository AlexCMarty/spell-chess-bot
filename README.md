# Spell Chess Bot

Goal: a bot that beats humans at [Spell Chess](https://www.chess.com/variants/spell-chess),
chess.com's chess variant with castable spells.

## Start here

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

**Only ever open `https://www.chess.com/variants/spell-chess/analysis`.** Other pages can
match you into a game against a human, which would be a terms-of-service violation.

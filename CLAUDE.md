This is a project to create a bot that can beat humans at the game of Spell Chess at `https://www.chess.com/variants/spell-chess/analysis`!

**Only ever open `https://www.chess.com/variants/spell-chess/analysis`.** Other pages can
match you into a game against a human, which would be a terms-of-service violation.

## Workspace

Three-crate Cargo workspace: `crates/core` (board, movegen, rules engine — no internal deps),
`crates/search` (search algorithms, depends on `core`), `crates/cli` (the `spellchess` binary,
depends on both). `cargo build --workspace` / `cargo test --workspace` cover everything; scope
to `-p spellchess-core` etc. to iterate on one crate.

## Ruleset

Full rule corpus lives in [`rules/`](rules/), routed from [`rules/INDEX.md`](rules/INDEX.md) —
read that first; it points to the exact file/anchor for any question. Rules are reverse-engineered
from chess.com's own client engine (`variants.js`) and verified by executing that engine, since
public chess.com documentation of this variant is thin and wrong in several places. Each rule
statement is tagged `[VERIFIED]`, `[CODE]`, `[DOC]`, or `[UNVERIFIED]` — when code and prose
disagree, code wins.

### The short version

- Each turn is **one optional spell + one mandatory piece move**. You can never pass.
- **Freeze** (5 per game, 3-turn cooldown): immobilises a 3×3 block, including the caster's own
  pieces. Frozen pieces exert **no control at all** — no check, no guarded squares — but still
  occupy their square and can still be captured.
- **Jump** (2 per game, 3-turn cooldown): makes one occupied square transparent to sliders, for
  **both players**. A check delivered through a jump square cannot be blocked.
- **The king can be captured, and it wins.** A spell cast at the start of your turn can create an
  attack the opponent never had a chance to answer.
- **Checkmate is not checkmate** while the defender still holds an unlocked spell that could
  produce a legal move.
- Spell counts never replenish; only the cooldown resets.

## Commits

You MUST use [Conventional Commits](https://www.conventionalcommits.org/) for every commit
message (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, etc.).

### Engine bundle

`research/variants.js` / `research/variants.pretty.js` are chess.com's own client engine, used
as a ground-truth oracle (see [`rules/70-engine-api.md`](rules/70-engine-api.md)). They are
third-party copyrighted code — **gitignored**, kept locally only, never committed or redistributed.
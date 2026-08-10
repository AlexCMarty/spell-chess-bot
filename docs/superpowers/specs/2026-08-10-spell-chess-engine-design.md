# Spell Chess move-advisor bot — design

Date: 2026-08-10
Status: approved

## Goal

Build a Spell Chess engine stronger than a human, delivered as a **local move-advisor
CLI**: you play the actual game yourself on chess.com, typing each move (yours and the
opponent's) into the tool as it happens. When it's your turn, you ask the tool for a
suggestion — including which spell to cast, if any — and play that move yourself on the
board. The bot never touches chess.com and never plays a live game itself; that would
violate chess.com's terms of service (see `CLAUDE.md`).

## Non-goals

- No browser automation of a live game, no chess.com API/account interaction at runtime.
- No UCI/lichess-bot-style protocol — a human is always relaying moves.
- No opening book / endgame tablebase in v1 — pure search + eval. Can be added later.
- No GUI — terminal REPL is sufficient.

## Architecture

Rust workspace, three crates, no runtime dependency on chess.com's engine
(`research/variants.js` is gitignored, third-party, dev-time-only — see `CLAUDE.md`).

```
crates/
  core/    — rules engine: board, legal move/turn generation, terminal detection
  search/  — alpha-beta search + evaluation, built on core
  cli/     — interactive move-advisor REPL, built on core + search
```

### `crates/core`

Owns all game-rule correctness. Key types:

- `Position` — 8×8 board, side to move, castling rights, en-passant target, halfmove/
  fullmove counters, per-side spell state `{ freeze: {count, lock}, jump: {count, lock} }`,
  live spell fields (`{ square, owner, kind, expires_after }`, at most a couple live at
  once per `rules/20-spell-system.md#field-lifetime`).
- `Turn` — `{ spell: Option<{kind, square}>, mv: Move }`. A turn is always exactly one
  mandatory move, with at most one optional spell cast first and resolved before the move,
  per `rules/20-spell-system.md#turn-structure`.

Core correctness primitive: `is_square_attacked(&Position, square, by_color) -> bool`.
This single function must:

- **exclude frozen pieces from attacking** — a frozen piece gives no check, guards no
  square, and pins nothing (`rules/30-freeze.md#frozen-pieces-exert-no-control`);
- **treat a live jump square as transparent to sliding attackers**, in both directions —
  an attacker's ray continues through it regardless of what stands there, which is what
  makes king-capture-via-jump legal and makes checks through a jump square unblockable
  (`rules/40-jump.md`).

Move legality is checked by brute force: generate pseudo-legal moves (frozen origin
squares produce none), apply spell field effects first, then for each candidate move
apply it and confirm the mover's own king is not attacked afterward using the same
`is_square_attacked`. This deliberately avoids hand-rolled pin/block-square tables —
getting those right for a variant with two novel control-altering effects is exactly
where a naive implementation goes wrong (see `rules/90-test-vectors.md` vector 4 and 12).
King capture requires no special-casing: it is simply a normal capturing move whose
target happens to be the enemy king, which is already produced by ordinary attack
generation once a spell has removed the piece that used to block that attack.

Terminal detection (`game_status`) implements the spell-escape-hatch guard from
`rules/50-interactions.md#the-spell-escape-hatch`: a side with zero ordinary legal moves
is only checkmated/stalemated if it *also* has no unlocked spell (`count > 0 && lock ==
0`) with a legal target that would create at least one ordinary legal move.

`core` defines its own compact save/debug notation; it does not implement chess.com's
14×14 FEN format — that's irrelevant at runtime and only used transiently by dev-time
oracle-diff tooling (see Testing).

### `crates/search`

- Negamax alpha-beta with iterative deepening, transposition table (Zobrist hash
  extended to include spell counts/locks/live fields, since those affect legality and
  eval, not just the board), MVV-LVA capture ordering plus killer/history heuristics,
  quiescence search on captures (including king captures, so the signature Spell Chess
  tactic isn't missed at the search horizon).
- Evaluation starts with material + piece-square tables + mobility, plus spell-aware
  terms: available-spell "tempo" value, and cheap tactical scans for one-turn
  freeze-mate and jump-capture threats per `rules/50-interactions.md#threat-detection-for-bots`.
- Two supported stopping modes, sharing one iterative-deepening driver:
  - **time-boxed** — default ~5s, configurable; returns the best move found when the
    clock expires.
  - **depth-boxed** — searches to a fixed ply count regardless of elapsed time.
- Known risk: turn branching factor is large whenever a spell is available (e.g. up to
  64 freeze targets × a regenerated move list each). This is addressed with move
  ordering / deprioritizing low-value casts (freezing empty, irrelevant squares), not by
  silently dropping legal moves — a correctness/performance tradeoff to tune once the
  engine is running, not a spec commitment to a specific heuristic today.

### `crates/cli`

Interactive REPL, offline only (no network/browser code):

| Command | Effect |
|---|---|
| `newgame [white\|black]` | Reset to the start position; set which side you're playing |
| `<turn>` | Apply a turn to the tracked position, e.g. `e2e4`, `freeze@d5 e2e4`, `e7e8q` |
| `go [--time Ns \| --depth N]` | Search from the current position, print suggested turn + eval + PV; does not commit it |
| `board` | Print an ASCII board + spell counts/cooldowns/live fields |
| `undo` | Pop the last applied turn (typo recovery) |
| `fen` | Dump the internal position string for debugging |
| `quit` | Exit |

Turn input uses coordinate notation (`<from><to>[promo]`, optionally prefixed with
`<spell>@<square> `) rather than full SAN, to avoid needing a SAN disambiguation parser.

## Testing strategy

**Tier 1 (required, blocks search work):** hand-encode all 20 vectors from
`rules/90-test-vectors.md` as `core` unit/integration tests. They already carry exact
engine-verified expected output and need no browser access. `core` must be fully green
here before `search` or `cli` work proceeds.

**Tier 2 (stretch, after Tier 1 is green):** use the chrome-devtools MCP tool against
`https://www.chess.com/variants/spell-chess/analysis` — and only that page, per the
safety constraint in `rules/70-engine-api.md` and `CLAUDE.md` — to run the differential
testing recipe already documented there against a broader/deeper set of positions.
Results are checked in as data fixtures (positions + expected legal-move sets), not as
oracle code; the oracle itself is never a build dependency.

**Strength validation:** since the bot cannot play a human via automation, engine
strength is sanity-checked via self-play — search+eval vs. a deliberately weak
material-only baseline, and games at different depths/time budgets against each other —
plus basic node-count sanity checks during development (no official Spell Chess perft
numbers exist, so shallow-depth counts are cross-checked against the live oracle instead).

## Build plan / sequencing

Real dependencies exist between crates, so work is staged rather than fully parallel:

1. `core`: board representation + orthodox move generation + basic geometry vectors
   (1, 3, 6, 7, 8, 15, 16, 17).
2. `core`: freeze/jump mechanics + the highest-risk correctness vectors (2, 4, 5, 9, 10,
   11, 12) — these are where a naive implementation is most likely to be wrong.
3. `core`: terminal detection + escape-hatch vectors (13, 14, 18, 19).
4. Once `core` is green, in parallel: `search` (alpha-beta/ID/TT/eval) and `cli`
   (REPL, depends only on `core`'s public types plus `search`'s public entry point).

Concrete task breakdown and subagent assignment happen in the implementation plan, not
in this spec.

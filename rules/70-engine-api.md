---
id: engine-api
title: Spell Chess — Driving the Real chess.com Engine as an Oracle
summary: A working JavaScript harness for running chess.com's own Spell Chess engine in the browser. Use it to validate a bot's move generator against ground truth and to re-verify any rule in this corpus.
keywords: [spell chess, engine, API, oracle, testing, getDestinations, play, multimoves, FEN builder, analysis board, chrome devtools, validation]
answers:
  - How do I test my Spell Chess move generator against the real rules?
  - How do I reach the engine object on the analysis board?
  - How do I build an arbitrary test position?
  - How do I cast a spell programmatically?
---

# Spell Chess — Using the Real Engine as an Oracle

> **Context:** Spell Chess is the chess.com 2-player variant. Orthodox chess plus two
> castable spells (**freeze**, **jump**). Every `[VERIFIED]` claim in this corpus was
> produced with the harness below. Use it to re-check any rule, and to differential-test a
> bot's move generator.

## Safety constraint

**Only ever use `https://www.chess.com/variants/spell-chess/analysis`.**

Navigating elsewhere on chess.com risks being matched into a live game against a human.
An automated agent playing a human on chess.com violates the site's terms of service and
gets accounts suspended. The analysis board is fully sufficient: it exposes the same engine,
in-process, with no server round-trip and no opponent.

## Reaching the engine

`[VERIFIED]` The page is a Vue 3 app. The engine instance lives on the `root` Pinia store as
`fpc` (four-player chess — the engine is shared across all chess.com variants).

```js
// Run in the page context of .../variants/spell-chess/analysis
const findApp = (n, d = 0) => {
  if (!n || d > 12) return null;
  if (n.__vue_app__ && n.id === 'app') return n.__vue_app__;
  for (const c of n.children) { const r = findApp(c, d + 1); if (r) return r; }
  return null;
};
const app    = findApp(document.body);
const pinia  = app.config.globalProperties.$pinia;
const engine = pinia._s.get('root').fpc;      // ← the engine
```

`[VERIFIED]` Do **not** call `engine.reset()` on this live instance — it empties the board
and the page will not recover without a reload. Build sandbox instances instead (below).

## Coordinate helpers

`[VERIFIED]` All engine input and output uses 14×14 coordinates.

```js
const to14   = s => String.fromCharCode(s.charCodeAt(0) + 3) + (parseInt(s.slice(1)) + 3);
const from14 = s => String.fromCharCode(s.charCodeAt(0) - 3) + (parseInt(s.slice(1)) - 3);
```

## Sandbox factory

`[VERIFIED]` Create throwaway engines so experiments never disturb the live board.

```js
const START = engine.fen();                                   // canonical start FEN
const Q     = JSON.parse(JSON.stringify(engine.getQ()));      // queue config (has ruleVariants)
const RV    = JSON.parse(JSON.stringify(engine.getRuleVariants()));

const mk = (fen) => {
  const g = engine.newFPC();
  g.setGameType('singles');
  g.setRuleVariants({ q: Q, ruleVariants: RV, applyStartFen: true });
  g.position(fen || START);
  return g;
};
```

## Position builder

`[VERIFIED]` Builds a valid extended FEN from an 8×8 piece map. Verified to reproduce the
canonical start position byte-for-byte.

```js
const buildFen = (pieces, opt = {}) => {
  const grid = Array.from({ length: 14 }, () => Array(14).fill('x'));
  for (let r = 1; r <= 8; r++) for (let f = 0; f < 8; f++) grid[11 - r][3 + f] = false;
  for (const [sq, pc] of Object.entries(pieces)) {
    grid[11 - parseInt(sq.slice(1))][3 + sq.charCodeAt(0) - 97] = pc;   // '0K', '2Q', …
  }
  const rows = grid.map(row => {
    const out = []; let empty = 0;
    for (const c of row) {
      if (c === false) empty++;
      else { if (empty) { out.push(String(empty)); empty = 0; } out.push(c); }
    }
    if (empty) out.push(String(empty));
    return out.join(',');
  });
  const flex = {
    pawnBaseRank: 5, wb: true, dim: '8x8',
    spells: [opt.wSpells || 'jump_0x2,freeze_0x5', '',
             opt.bSpells || 'jump_0x2,freeze_0x5', ''],
    spellFields: opt.spellFields || []
  };
  return [
    rows.join('/'), String(opt.turn || 0),
    JSON.stringify([false, true, false, true]),
    JSON.stringify(opt.castleKS || [false, false, false, false]),
    JSON.stringify(opt.castleQS || [false, false, false, false]),
    JSON.stringify([0, 0, 0, 0]), String(opt.plies || 0),
    JSON.stringify(flex)
  ].join('-');
};
```

## Querying and playing

```js
// Legal destinations of the piece on an 8x8 square
const dests = (g, sq) => (g.getDestinations(to14(sq)) || []).map(from14);

// Legal targets for a spell ('freeze' | 'jump') for the side to move
const spellTargets = (g, type) =>
  (g.getDestinations(`@@${g.turn()}_${type}`, false, false, false, true) || []).map(from14);

// Play a full turn: optional spell + mandatory move
const turn = (g, spell /* 'freeze@d5' | null */, move /* 'e2e4' */) => {
  const step = { from: to14(move.slice(0, 2)), to: to14(move.slice(2, 4)) };
  if (!spell) return g.play(step, {});
  const [type, sq] = spell.split('@');
  return g.play({ multimoves: [
    { from: `@@${g.turn()}_${type}`, to: to14(sq) },
    step
  ]}, {});
};
```

`[VERIFIED]` `play()` returns `null` for an illegal move and a move object otherwise. Useful
fields on that object: `san` (14×14, `&`-joined), `san8x8` (8×8, space-joined), `sanShort`.

> `[VERIFIED]` **The raw API is more permissive than the rules.** `play()` will accept a
> spell with no following move, and a `multimoves` array containing two spells. Neither is
> legal play — the 2-phase limit is imposed by the client UI. Your bot must enforce it.
> See [`20-spell-system.md#turn-structure`](20-spell-system.md#turn-structure).

## Inspecting state

| Call | Returns |
|---|---|
| `g.turn()` | `0` (White) or `2` (Black) |
| `g.fen()` | Extended FEN |
| `g.spells(player)` | `{ jump: {count, lock}, freeze: {count, lock} }` |
| `g.spellFields()` | Live fields: `[{ target, player, type, life }]` |
| `g.isFrozen(sq14)` | Whether a 14×14 square is inside a live freeze zone |
| `g.inCheck()` | Check status |
| `g.isInCheckmate(player)` / `g.isInStalemate(player)` | Predicates (spell-aware) |
| `g.gameOver()` / `g.termination()` | `true` / `"Checkmate"`, `"Stalemate • Draw"`, … |
| `g.getWinner()` | Winning seat |
| `g.getSpellLock()` / `g.getSpellFieldLife()` / `g.isSpellSacredRoyal()` | Config: `3` / `1` / `false` |
| `g.getRuleVariants()` | Full rule-variant object |

`[VERIFIED]` For terminal detection prefer `gameOver()` + `termination()` over
`isInCheckmate()`: once a game-ending move has been played, `gameOver()` is authoritative,
while `isInCheckmate()` can read `false` for an already-finished game.

## Differential testing recipe

The highest-value use of this harness is to prove a bot's move generator correct:

```js
// For each test position and each side to move:
//   1. engineMoves = for every square, dests(g, sq)  (+ spellTargets for both spells)
//   2. botMoves    = your generator's output for the same FEN
//   3. assert setEquals(engineMoves, botMoves)
// Then repeat with each legal spell pre-cast, since a spell changes the move set.
```

`[VERIFIED]` Remember to regenerate ordinary moves **after** applying a candidate spell —
freeze can remove your own options and jump adds options for both sides.

## Source of the engine

`[VERIFIED]` The bundle is served from:

```
https://www.chess.com/r2/client-packages/variants/<version>/variants.js
```

Version at time of writing: `2026.8.1`. It is minified; run it through a beautifier before
reading. The spell logic lives in the large engine closure — search for `spellChess`,
`spellFields`, and the functions that compute the 3×3 zone and the spell target lists.

## See also

- Notation and FEN details → [`60-notation-and-encoding.md`](60-notation-and-encoding.md)
- Ready-made positions with expected output → [`90-test-vectors.md`](90-test-vectors.md)

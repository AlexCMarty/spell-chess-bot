---
id: overview
title: Spell Chess — Overview, Canonical Configuration, Coordinate Model
summary: What Spell Chess is, the exact rule configuration used by live chess.com games and the analysis board, the 8x8-in-14x14 coordinate system, and a list of errors in chess.com's public documentation.
keywords: [spell chess, overview, configuration, Spell=1+3, sacredRoyal, coordinates, 14x14, player index, freeze count, jump count]
answers:
  - What is Spell Chess?
  - What is the canonical rule configuration?
  - How many freezes and jumps does each player start with?
  - How do 8x8 squares map to the engine's 14x14 grid?
  - Which chess.com documentation statements are wrong?
---

# Spell Chess — Overview

> **Context:** Spell Chess is the chess.com 2-player variant at
> `https://www.chess.com/variants/spell-chess`. Orthodox chess plus two castable spells
> (**freeze**, **jump**). This file establishes the canonical configuration and coordinate
> model that every other file in this corpus assumes.

## What the game is

`[DOC]` chess.com's own in-client description, read from the game-queue metadata
(`queue.description`), is the single most accurate prose statement of the rules:

> "Cast a spell before making a move. Spells are limited, recharge after 3 full turns, and
> you cannot cast the same spell two moves in a row. Use the Jump spell on another piece to
> hop over like it isn't there. Or use the Freeze spell to prevent pieces from moving or
> checking within a 3x3 area of effect. Be careful—any piece, including your own, will
> freeze if it enters the spell area. Use spells to find a checkmate or king capture!"

`[VERIFIED]` Every clause of that paragraph is confirmed by the engine, with one important
qualification: "any piece will freeze if it enters the spell area" is true only for the
lifetime of the field, which in the canonical configuration is a single turn. See
[`30-freeze.md#entering-a-zone`](30-freeze.md#entering-a-zone).

## Canonical configuration

`[VERIFIED]` These are the values reported by the live engine on the analysis board and by
the default matchmaking queue ("Spell Chess", 5|2 blitz, rated).

| Setting | Value | Engine field |
|---|---|---|
| Rule-variant string | `EnPassant Play4Mate Spell=1+3` | PGN `[RuleVariants]` |
| Spell parameter | `Spell=1+3` | `ruleVariants.spellChess === "1+3"` |
| Spell field duration | **1 turn** | `getSpellFieldLife() === 1` |
| Spell cooldown ("lock") | **3 turns** | `getSpellLock() === 3` |
| Sacred Royal | **off** | `isSpellSacredRoyal() === false` |
| Starting freezes | **5 per player** | FEN `spells: "…freeze_0x5"` |
| Starting jumps | **2 per player** | FEN `spells: "jump_0x2…"` |
| En passant | on | `ruleVariants.enPassant === true` |
| Play-for-mate | on | `ruleVariants.play4mate === true` |
| Board | 8×8, standard start | `dim === "8x8"` |
| Players | 2 (White, Black) | `twoPlayer() === true` |

### Reading the `Spell=` parameter

`[CODE]` The parameter is parsed by the regex `/([1-9][0-9]*)\+([1-9][0-9]*)(!)?/`:

```
Spell = <fieldLife> + <lock> [!]
         │            │       └── "!" present  ⇒ sacredRoyal ON
         │            └────────── cooldown in turns after casting
         └─────────────────────── how many turns a cast field stays on the board
```

So canonical `Spell=1+3` means: **fields last 1 turn, cooldown is 3 turns, sacredRoyal
off.** Spell *counts* (5 freeze / 2 jump) are **not** encoded here — they live in the FEN.
See [`60-notation-and-encoding.md#fen-spell-encoding`](60-notation-and-encoding.md#fen-spell-encoding).

`[CODE]` The 4-player Spell Chess default is `Spell=2+5`. This corpus documents the
2-player game; treat 4-player behaviour as out of scope.

### What `sacredRoyal` is

`[CODE]` `sacredRoyal` is an optional toggle, **off** in the canonical game, exposed in the
custom-position editor as "Sacred Royal". The client's own tooltip reads:

> "You can freeze a king, but if there is a check, the royal blood becomes hot and dispels
> frost"

Meaning: when sacredRoyal is **on**, a king that is frozen *and* in check is exempted from
the freeze and may move to escape. `[CODE]` In the move generator, a frozen square yields
no destinations unless `sacredRoyal && the player is in check && the piece is that player's
king`.

**Because sacredRoyal is OFF in the canonical game, freezing the enemy king is a fully
effective mating tool** — a frozen king in check cannot move, and if the defender has no
other resource the game ends. See
[`50-interactions.md#freeze-mate`](50-interactions.md#freeze-mate).

## Coordinate model

`[VERIFIED]` The engine is a 4-player-chess engine reused for 2-player variants. It always
works on a **14×14 grid**, with the 8×8 board embedded in the middle. Off-board cells are
the literal token `x`.

- 8×8 file `a…h` ⇒ 14×14 file `d…k` (**+3**)
- 8×8 rank `1…8` ⇒ 14×14 rank `4…11` (**+3**)

```
8x8   ->  14x14          14x14 -> 8x8
a1    ->  d4             d4    -> a1
e1    ->  h4             h4    -> e1
e8    ->  h11            h11   -> e8
d5    ->  g8             g8    -> d5
h8    ->  k11            k11   -> h8
```

```js
const to14   = s => String.fromCharCode(s.charCodeAt(0) + 3) + (parseInt(s.slice(1)) + 3);
const from14 = s => String.fromCharCode(s.charCodeAt(0) - 3) + (parseInt(s.slice(1)) - 3);
```

`[VERIFIED]` This matters constantly: the engine's `san` field and all `getDestinations`
input/output use **14×14** coordinates. A parallel `san8x8` field carries human-readable
8×8 notation. Confusing the two is the single easiest way to misread engine output.

### Player indices

`[VERIFIED]` Seats are 0–3. In 2-player Spell Chess:

| Index | Colour | FEN piece prefix | Status |
|---|---|---|---|
| 0 | White | `0` (e.g. `0K`) | active |
| 1 | — | — | dead/unused |
| 2 | Black | `2` (e.g. `2K`) | active |
| 3 | — | — | dead/unused |

Turn order is 0 → 2 → 0 → 2. `turn()` returns `0` or `2`, never `1` or `3`.

## Known errors in public sources

`[VERIFIED]` chess.com's public help pages contradict the engine. Do not use them.

| Source claim | Reality |
|---|---|
| "Any piece in that area cannot move **for the rest of that turn**" (chess.com/terms) | The field lasts through the **opponent's** following turn, then expires. → [`20`](20-spell-system.md#field-lifetime) |
| "Pieces within this area cannot move **during your opponent's next turn**" (help centre) | It also restricts **your own** pieces during **your own** move, in the same turn you cast it. → [`30`](30-freeze.md#the-zone-affects-the-caster-too) |
| "Jump spells allow a player to jump over a specific piece" | The field is attached to a **square** and is usable by **both** players. → [`40`](40-jump.md#who-can-use-the-field) |
| "players must wait for three turns before they can use the same spell again" | Correct, but incomplete: a spell on cooldown also **cannot rescue you from checkmate**. → [`50`](50-interactions.md#the-spell-escape-hatch) |
| Various third-party pages: "the jump spell allows Bishops, Rooks and Queens to jump over one piece" | Directionally right, but the mechanism is square transparency, which also enables pawn double-steps and jump-through checks. → [`40`](40-jump.md) |

`[VERIFIED]` The chess.com/terms page says wins come from "capturing the enemy king". The
in-client description says "checkmate **or** king capture". The latter is correct: both
occur, and the engine reports both as termination `"Checkmate"`.

## Where to go next

- Turn structure and spell economy → [`20-spell-system.md`](20-spell-system.md)
- The two spells in detail → [`30-freeze.md`](30-freeze.md), [`40-jump.md`](40-jump.md)
- Win conditions and edge cases → [`50-interactions.md`](50-interactions.md)

---
id: spell-system
title: Spell Chess — Turn Structure, Spell Economy, Cooldowns, Field Lifetime
summary: How a Spell Chess turn is composed, how many spells you get, how the 3-turn cooldown counts, and exactly how long a cast spell field stays on the board.
keywords: [spell chess, turn structure, multimove, one spell per turn, cooldown, lock, recharge, spell count, field lifetime, spell duration, casting]
answers:
  - How many spells can I cast per turn?
  - Do I have to move a piece after casting a spell?
  - When can I cast the same spell again?
  - How long does a freeze or jump field last?
  - Do spell counts regenerate?
---

# Spell Chess — The Spell System

> **Context:** Spell Chess is the chess.com 2-player variant. Orthodox chess plus two
> castable spells (**freeze**, **jump**). This file covers the mechanics common to both
> spells: when you may cast, what it costs, and how long the effect lasts.

## Turn structure

`[VERIFIED]` **A turn consists of exactly two phases:**

```
turn := [ optional: cast one spell ] , [ mandatory: one ordinary piece move ]
          phase 0                        phase 1
```

- `[VERIFIED]` **The piece move is mandatory.** You cannot pass, and casting a spell is not
  a turn by itself. The client models the turn as a 2-step multimove (`Ve === 2`) in which
  step 0 may be skipped but step 1 may not.
- `[VERIFIED]` **At most one spell per turn.** Phase 0 accepts a single cast.
- `[VERIFIED]` **The spell resolves first, before your move**, and its effects are already
  in force while you make that move. This is not a technicality — it constrains you. See
  [`30-freeze.md#the-zone-affects-the-caster-too`](30-freeze.md#the-zone-affects-the-caster-too).
- `[VERIFIED]` You may cast **freeze on one turn and jump on the next**; the two spells have
  fully independent cooldowns.

> **Implementation note.** `[VERIFIED]` The raw engine function `play()` will accept a
> spell with no following move, and will even accept two spells in one call, because that
> low-level API performs no phase checking. **Neither is legal play.** The 2-phase limit is
> enforced by the client that constructs moves. A bot must enforce it itself; do not infer
> the rules from what `play()` tolerates.

### Move generation order

`[CODE]` The engine's own move generator composes a turn exactly this way:

```js
let spell = maybeChooseSpell(player);          // may be undefined
let moves = generateRegularMoves();            // generated AFTER the spell is applied
if (!moves.length && spellChess && !spell) {   // no legal move without help?
  spell = chooseSpell(player, /*force*/ true); // try a spell to create one
  moves = generateRegularMoves();
}
const turn = spell ? { multimoves: [spell, move] } : move;
```

Two consequences a bot must respect:

1. **Legal ordinary moves must be regenerated after the spell is applied.** A freeze can
   remove your own options; a jump can add options for both sides.
2. **A spell may be cast specifically to create a legal move** when you would otherwise
   have none. This is the mechanism behind
   [`50-interactions.md#the-spell-escape-hatch`](50-interactions.md#the-spell-escape-hatch).

## Spell economy

`[VERIFIED]` Each player begins with:

| Spell | Starting count | Cooldown on cast |
|---|---|---|
| freeze | **5** | 3 turns |
| jump | **2** | 3 turns |

Per-spell state is `{ count, lock }`.

- `[VERIFIED]` **Counts never regenerate.** Casting decrements the count permanently
  (5 → 4 → 3 …). The word "recharge" in chess.com's description refers to the *cooldown*,
  not the count. Verified over a full cooldown cycle: the count stayed at 4 and did not
  return to 5.
- `[VERIFIED]` A cast is legal only if **`count > 0` AND `lock === 0`**. A cast with
  `count === 0` is rejected. A cast with `lock > 0` is rejected.
- `[VERIFIED]` Casting sets that spell's `lock` to **3** and decrements its `count` by 1.
  The other spell's lock is untouched.

### Cooldown timing

`[VERIFIED]` `lock` decrements by 1 once per full move cycle — measured, the decrement
lands at the end of the opponent's turn. Ply-by-ply trace of White casting freeze on
White's turn *N*:

| Point in game | White `freeze.lock` |
|---|---|
| immediately after casting (turn *N*) | 3 |
| after Black's reply to turn *N* | 2 |
| after White's turn *N+1* | 2 |
| after Black's reply | 1 |
| after White's turn *N+2* | 1 |
| after Black's reply | 0 |
| White's turn *N+3* | **0 → castable** |

**Rule of thumb:** having cast a spell on your turn *N*, you may cast it again on your turn
***N+3***. Turns *N+1* and *N+2* are blocked. This matches chess.com's "recharge after 3
full turns" and "you cannot cast the same spell two moves in a row".

### Cooldown is not just an inconvenience

`[VERIFIED]` A spell that is on cooldown **cannot save you from checkmate**. A position
where the defender holds 5 freezes and 2 jumps, all locked, was adjudicated
`termination: "Checkmate"` immediately. See
[`50-interactions.md#the-spell-escape-hatch`](50-interactions.md#the-spell-escape-hatch).

## Field lifetime

`[VERIFIED]` Casting a spell places a **field** on the board: a record of
`{ target, player, type, life }` with `life = 1` in the canonical configuration.

A field cast on turn *N* is active:

- for the remainder of turn *N* (i.e. **during the caster's own move**), and
- for the whole of the opponent's turn *N+1*,

and is **gone by the time the caster moves again**. Measured: after White cast freeze and
moved, `spellFields` had one entry; after Black's next turn, `spellFields` was empty.

```
White turn N :  cast freeze ──► field ACTIVE ──► White moves (constrained by it)
Black turn N :                  field ACTIVE ──► Black moves (constrained by it)
White turn N+1:                 field GONE
```

`[VERIFIED]` So a freeze restricts **exactly one enemy turn**, plus the tail of your own.
Multiple fields can coexist — for example your live jump field and your opponent's live
freeze field — and they are evaluated independently.

`[CODE]` `life` is the `<fieldLife>` half of the `Spell=<fieldLife>+<lock>` parameter. In a
hypothetical `Spell=2+…` game, fields would persist one extra turn and the "pieces entering
the zone become frozen" clause would start to matter; in the canonical game it effectively
does not. See [`30-freeze.md#entering-a-zone`](30-freeze.md#entering-a-zone).

## Casting API shape

`[VERIFIED]` A spell is expressed as a pseudo-move from a synthetic source square:

```
from: "@@<player>_<type>"     e.g. "@@0_freeze", "@@2_jump"
to:   <target square, 14x14>  e.g. "g8"  (= d5 on the 8x8 board)
```

A complete turn is submitted as a multimove:

```js
engine.play({ multimoves: [
  { from: "@@0_freeze", to: "g8"          },   // phase 0 — optional
  { from: "h5",         to: "h7"          }    // phase 1 — mandatory
]});
```

Full details and a working harness: [`70-engine-api.md`](70-engine-api.md).

## See also

- What freeze does → [`30-freeze.md`](30-freeze.md)
- What jump does → [`40-jump.md`](40-jump.md)
- How spells change win/draw adjudication → [`50-interactions.md`](50-interactions.md)

---
id: jump
title: Spell Chess — The Jump Spell (complete)
summary: Complete rules for the jump spell. Square transparency, legal targets, the fact that both players benefit, pawn double-steps over a jumped blocker, unblockable checks through a jump square, and king capture.
keywords: [spell chess, jump, transparency, hop over piece, slider, x-ray, unblockable check, pawn double step, king capture, discovered attack]
answers:
  - What does the jump spell actually do?
  - Which squares can I target with jump?
  - Can my opponent use the jump field I created?
  - Can a knight or king benefit from jump?
  - Can a check through a jump square be blocked?
  - How do I capture the king with jump?
---

# Spell Chess — The Jump Spell

> **Context:** Spell Chess is the chess.com 2-player variant. Orthodox chess plus two
> castable spells (**freeze**, **jump**). This file is the complete specification of
> **jump**. Each player starts with **2**; cooldown 3 turns; the field lasts 1 turn.

## The core mechanic: square transparency

`[VERIFIED]` Jump places a field on **one square**. While that field is live, the square is
**transparent for line-of-sight purposes**: sliding pieces trace their rays straight through
it as if it were empty.

`[VERIFIED]` The critical modelling point: **the field belongs to the square, not to the
piece standing on it, and not to the caster.** It is a property of the board.

## Targeting

`[VERIFIED]` **Jump may target any square that currently contains a piece.** Empty squares
are not legal targets — this is the one place jump is more restrictive than freeze.

`[VERIFIED]` Any piece qualifies: yours or your opponent's, any type, **including either
king**. Measured target list on a test position: `[a1, a8, b4, d2, e1, e8, h2]` — every
occupied square, both kings included, nothing else.

| | freeze | jump |
|---|---|---|
| Empty square | `[VERIFIED]` legal target | `[VERIFIED]` **illegal** target |
| Own piece | `[VERIFIED]` legal | `[VERIFIED]` legal |
| Enemy piece | `[VERIFIED]` legal | `[VERIFIED]` legal |
| Either king | `[VERIFIED]` legal | `[VERIFIED]` legal |
| Square already carrying a **live jump field** | `[VERIFIED]` legal (unaffected — freeze and jump exclusions are independent) | `[VERIFIED]` **illegal** — see below |

`[VERIFIED]` **A square that already has a live jump field on it is not a legal jump
target, for either player.** Measured: White cast `jump@c5` (a knight there); on Black's
very next turn — before the field expires — `c5` was absent from Black's jump target
list, even though it's still occupied and otherwise a completely ordinary target. The
exclusion is by square and spell type only, independent of who owns the existing field or
who is asking: it is not "you can't recast your own field" or "opponents can't target your
field," it is "one live jump field per square, period." A square may still carry a live
freeze field and be jump-targeted (or vice versa) — the two spell types don't block each
other, only same-type recast on the same square is excluded. Confirmed with the browser
oracle (`rules/70-engine-api.md`); see `crates/core/tests/fixtures/field_jump_01_discovered.json`
and `field_jump_02_self.json`.

## Who can use the field

`[VERIFIED]` **Both players.** While the field is live, *any* sliding piece of *either*
colour sees through that square.

Measured: White cast `jump@d4` on White's **own** pawn. On the very next turn Black's rook
on `d8` had destinations `d7 d6 d5 d4 d3 d2 d1` — it sliced straight through the white pawn,
all the way down the board. White's own rook on `d1` likewise gained `d5 d6 d7 d8`.

This makes jump genuinely double-edged, and it is the point most often stated wrongly in
public write-ups ("allows *a player* to jump over a piece"). A bot must evaluate the
opponent's replies with the field still active.

## Which pieces benefit

`[VERIFIED]` Only movement that depends on line of sight is affected:

| Piece | Benefits from a jump field? |
|---|---|
| Queen, Rook, Bishop | `[VERIFIED]` **Yes** — rays pass through the square |
| Pawn (double-step) | `[VERIFIED]` **Yes** — see below |
| Pawn (single push, captures) | `[VERIFIED]` No — adjacent, nothing to see past |
| Knight | `[VERIFIED]` No — already ignores intervening pieces |
| King | `[VERIFIED]` No — moves one square |

### Pawn double-step

`[VERIFIED]` A pawn on its home rank may double-step **over a jumped blocker** occupying the
intermediate square.

Measured: white pawn `d2`, black rook `d3`. Without a spell the pawn had **no** moves. After
`jump@d3`, the pawn's destinations became `[d4]` — it hopped the rook. It still could not
move to `d3` itself, since pawns do not capture straight ahead.

### You can still capture the piece on the jump square

`[VERIFIED]` Transparency is additive, not substitutive. The jumped square remains a normal
occupied square: a slider may either capture the piece standing on it **or** pass through to
squares beyond. Measured: a rook's destination list under a live jump field contained both
the jump square and every square behind it.

## Checks through a jump square are unblockable

`[VERIFIED]` If a check is delivered along a ray that passes through a live jump square,
**the defender cannot interpose on that square**, and cannot interpose behind it either.

`[CODE]` The engine explicitly filters candidate blocking squares, removing any square
carrying a live jump field that will still be active.

`[VERIFIED]` The defender's only legal answers are therefore:

1. move the king, or
2. capture the checking piece.

Measured: with Black's bishop on `b4` checking `Ke1` through a jump field on `d2`, White's
rook on `a1` had **zero** legal moves (no interposition available), while the king had
`d1 e2 f1 f2` and White's bishop on `d2` could capture the checker on `b4`.

## King capture — the signature tactic

`[VERIFIED]` This is how Spell Chess games actually end, and it is the reason the variant
allows king capture at all.

The mechanism: **you cast the jump at the start of your own turn, then immediately exploit
it.** The attack it creates did not exist when your opponent moved, so they never had an
opportunity to answer it — and you take the king in the same turn.

### Worked example (exactly reproduced from a real game)

```
Position:  White Ke1, Bd2, Ra1, Rh2      Black Ke8, Bb4, Ra8      Black to move
```

White's bishop on `d2` blocks the `b4–e1` diagonal, so `Bb4` is not attacking the king and
White stands perfectly safe. Black plays, as one turn:

```
jump@d2   +   Bb4xe1
```

The jump makes `d2` transparent; the bishop slides `b4 → c3 → (d2) → e1` and captures the
king.

`[VERIFIED]` Engine output for that exact turn:

```
san      : "jump@g5&Be7xKh4#"        (14x14 coordinates)
san8x8   : "jump@d2 Bxe1#"           (8x8 coordinates)
gameOver : true
termination : "Checkmate"
winner   : 2   (Black)
```

`[VERIFIED]` Note the `xK` in `sanShort` (`"jump@g5 BxKh4#"`) — the engine marks a literal
king capture, and scores the result as `"Checkmate"`.

**Defensive corollary for a bot:** before moving, you must ask not only "am I in check?" but
**"could my opponent create a check-and-capture in one turn using a spell?"** Any enemy
slider that would attack your king if exactly one intervening square were removed is a
live threat, provided the opponent has a jump available (`count > 0`, `lock === 0`).
See [`50-interactions.md#threat-detection-for-bots`](50-interactions.md#threat-detection-for-bots).

## Interaction summary

| Question | Answer |
|---|---|
| Can jump target an empty square? | `[VERIFIED]` **No** |
| Can jump target a king? | `[VERIFIED]` Yes |
| Does my opponent benefit from my jump field? | `[VERIFIED]` **Yes** |
| Does jump help a knight or king? | `[VERIFIED]` No |
| Can a pawn double-step over a jumped blocker? | `[VERIFIED]` Yes |
| Can I still capture the piece on the jump square? | `[VERIFIED]` Yes |
| Can a check through a jump square be blocked? | `[VERIFIED]` **No** |
| Can I capture the king with it? | `[VERIFIED]` Yes — and it wins |
| How long does it last? | `[VERIFIED]` My move + the opponent's next turn |
| Does it stack with a freeze field? | `[VERIFIED]` Yes, fields are independent |

## See also

- Cooldown and counts → [`20-spell-system.md#spell-economy`](20-spell-system.md#spell-economy)
- The freeze spell → [`30-freeze.md`](30-freeze.md)
- Win conditions in full → [`50-interactions.md#win-conditions`](50-interactions.md#win-conditions)
- Reproducible positions → [`90-test-vectors.md`](90-test-vectors.md)

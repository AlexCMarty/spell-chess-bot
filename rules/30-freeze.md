---
id: freeze
title: Spell Chess — The Freeze Spell (complete)
summary: Complete rules for the freeze spell. Targeting, 3x3 zone geometry with edge clipping, what a frozen piece can and cannot do, the fact that frozen pieces exert no control, escaping check by freezing the checker, and interaction with the caster's own move.
keywords: [spell chess, freeze, 3x3 zone, frozen piece, area of effect, edge clipping, dispel check, freeze checker, frozen king, control, guarded squares]
answers:
  - Which squares can I target with freeze?
  - What shape is the freeze zone at the board edge or corner?
  - Can a frozen piece give check or guard squares?
  - Can I capture a frozen piece?
  - Can I escape check by freezing the checking piece?
  - Does my own freeze restrict my own pieces?
---

# Spell Chess — The Freeze Spell

> **Context:** Spell Chess is the chess.com 2-player variant. Orthodox chess plus two
> castable spells (**freeze**, **jump**). This file is the complete specification of
> **freeze**. Each player starts with **5**; cooldown 3 turns; the field lasts 1 turn.

## Targeting and geometry

`[VERIFIED]` **Freeze may target any square of the 8×8 board** — occupied or empty, your
piece or the opponent's — **subject to the two exclusions below**: a square already
carrying a live freeze field, and any cast that would leave the mover with zero legal
moves. The engine's target list for freeze at the start position is all 64 squares,
because neither exclusion applies there. Do not lift "all 64" into a move generator as
an unconditional rule.

`[VERIFIED]` The effect is the **3×3 block of squares centred on the target, clipped to the
board.** Squares outside the 8×8 board are simply not part of the zone.

`[VERIFIED]` **A square that already has a live freeze field anchored on it is excluded
from the freeze target list**, for either player, until that field expires — the mirror of
jump's same-square exclusion (see [`40-jump.md`](40-jump.md#targeting)). Measured: White
cast `freeze@c5`; on Black's very next turn the freeze target list was all 64 squares
*except* `c5`. This is about the exact anchor square only, not the 3×3 zone it produces —
a square merely *inside* someone's freeze zone (but not itself an anchor) is still a legal
freeze target. The exclusion is independent of caster: it is "one live freeze field per
square," not "you can't recast your own." A jump-field anchor on a square does not block
freeze there, or vice versa. Confirmed with the browser oracle
(`rules/70-engine-api.md`); see `crates/core/tests/fixtures/field_freeze_01_corner.json`.

`[VERIFIED]` **A freeze target is also excluded from the target list if casting it would
leave the mover with zero legal moves anywhere on the board.** A turn is spell-plus-
mandatory-move; if every one of the mover's own pieces would end up frozen (or otherwise
immobile) by the cast, there's no way to complete the mandatory move half of the turn, so
the engine doesn't offer that square as a target at all — this is checked *before* move
generation, not surfaced as "you have no moves" after the fact. Measured: a sparse position
with Black to move holding only a king on `f6` and a pawn on `g4` (nothing else on the
board for Black) had a 62-square freeze target list — every square *except* `f5` and `g5`,
the only two centers whose 3×3 zone covers both `f6` and `g4` simultaneously, freezing
Black's entire army in one cast. See `crates/core/tests/fixtures/sparse_03.json`.

The same exclusion applies to jump — see [`40-jump.md#targeting`](40-jump.md#targeting).
Note that `spells::freeze_targets`/`jump_targets` do **not** model it, so a target list must
be derived from `generate_turns`'s output rather than from those functions directly.

| Target | Zone shape | Squares |
|---|---|---|
| Centre, e.g. `d5` | 3×3 (9) | c4 c5 c6, d4 d5 d6, e4 e5 e6 |
| Edge, e.g. `a5` | 2×3 (6) | a4 a5 a6, b4 b5 b6 |
| Corner, e.g. `a1` | 2×2 (4) | a1 a2, b1 b2 |

`[VERIFIED]` Corner clipping confirmed directly: with a freeze on `a1`, pieces on `a1` and
`b2` reported frozen while a piece on `c3` did not.

```
target = d5                target = a1
   c  d  e                    a  b  c
6  #  #  #                 3  .  .  .
5  #  ⊛  #                 2  #  #  .
4  #  #  #                 1  ⊛  #  .
   ⊛ = target,  # = also frozen
```

## Effects on a frozen piece

`[VERIFIED]` A piece standing on a frozen square **has zero legal moves**. It cannot move,
cannot capture, cannot castle, cannot promote, cannot be used to block a check.

`[VERIFIED]` This applies to **every** piece in the zone regardless of owner — the
opponent's pieces, and your own.

`[VERIFIED]` The check is on the piece's **origin square**. A piece whose current square is
frozen cannot move; a piece elsewhere may freely move *into* the zone.

### Frozen pieces exert no control

`[VERIFIED]` **This is the most important and least documented freeze rule.** A frozen piece
does not attack, guard, check, or pin anything. It is inert for every purpose except
occupying its square.

Measured: a black king on `d5` normally could not step to `c4`, `d4` or `e4` because a white
rook on `h4` guarded rank 4. After that rook was frozen, the king's legal destinations grew
to include `c4`, `d4` and `e4`.

Consequences a move generator must implement:

- A frozen piece **gives no check**.
- A frozen piece **does not guard squares**, so the enemy king may legally step onto squares
  it "attacks", and may legally capture a piece that only the frozen piece defends.
- A frozen piece **cannot pin**; a piece it was pinning is free.
- `[CODE]` The engine implements this by excluding frozen squares from attacker detection.

### A frozen piece still occupies its square

`[VERIFIED]` Freezing does not make a piece intangible. A frozen piece:

- **still blocks sliding pieces.** Measured: a white rook on `d1` with a frozen black knight
  on `d5` reached `d2 d3 d4 d5` and no further.
- **can still be captured**, entirely normally. Measured: `Rxd5` onto a frozen knight was
  legal.
- **can still be captured en passant** if it is a pawn. Measured: with a black pawn on `c5`
  frozen (and the capturing white pawn on `d5` *not* frozen), `dxc6` en passant was legal.
- still counts for material, still blocks castling if it stands between king and rook.

### Frozen kings

`[VERIFIED]` With `sacredRoyal` **off** (the canonical setting), a frozen king is frozen
like any other piece — **including while it is in check**. It cannot move to escape.

This makes freeze a direct mating weapon: freeze the enemy king and give check. If the
defender has no other resource, the game ends. Worked example:
[`50-interactions.md#freeze-mate`](50-interactions.md#freeze-mate).

`[CODE]` With `sacredRoyal` **on** (not the canonical game), a frozen king that is in check
is exempted and may move.

## Freezing a checker dispels the check

`[VERIFIED]` Because a frozen piece gives no check, **freezing the piece that is checking you
removes the check outright.** You are then free to play *any* legal move; you are under no
obligation to address the (now nonexistent) threat.

Verified sequence:

```
Position: white Ke1, Rd8, Ra1 ; black Ke8, Rh8 ; Black to move.
Black is in check from Rd8.

Black plays:  freeze@d8  +  Rh8-h7        ← legal; the rook move ignores the check entirely
Engine SAN:   "freeze@d8 Rh7"
```

`[VERIFIED]` On the following turn the frozen rook has no moves, so it cannot capture the
king either. The field then expires and the check becomes live again — so this is a
**one-turn reprieve, not a solution.** A bot must not treat "freeze the checker" as
resolving a threat; it defers it by exactly one turn.

## The zone affects the caster too

`[VERIFIED]` The freeze resolves **before** your own move in the same turn, and your own
pieces in the zone are frozen immediately.

- `[VERIFIED]` **You cannot move a piece that your own freeze just froze.** Measured:
  casting `freeze@d4` and then attempting to move the knight standing on `d4` was rejected.
- `[VERIFIED]` **You may move a different piece into the zone on the same turn.** Measured:
  casting `freeze@d5` and then playing a knight `f5-d6` (into the zone) was legal, SAN
  `"freeze@d5 Nd6"`.

So the rule is strictly about the **origin** square, for both players.

## Entering a zone

`[VERIFIED]` A piece may always move **into** a frozen zone. It is not prevented, and it is
not "caught".

`[DOC]` chess.com warns "any piece, including your own, will freeze if it enters the spell
area". `[VERIFIED]` This is accurate but nearly vacuous in the canonical game: because the
field's life is 1 turn, it expires at the end of the opponent's turn. Measured: a black
knight moved into a white freeze zone; on Black's next turn the field was gone and the
knight had its full set of moves.

With the canonical `fieldLife = 1` this clause has **no observable effect at all**: a
piece that enters a zone has already spent its move, and the field expires before it
could move again. It only bites in non-canonical configurations with `fieldLife ≥ 2`.

Do **not** conflate it with [the zone affecting the caster](#the-zone-affects-the-caster-too).
That rule is strictly about a piece's **origin** square. Freezing by *destination* is not
something the engine does, and a generator that implements it will reject legal moves.

## Interaction summary

| Question | Answer |
|---|---|
| Can freeze target an empty square? | `[VERIFIED]` Yes — all 64 *minus* live freeze anchors and any cast that would leave you with no legal move |
| Can freeze target the enemy king? | `[VERIFIED]` Yes |
| Does it freeze my own pieces? | `[VERIFIED]` Yes, including during my own move |
| Can a frozen piece be captured? | `[VERIFIED]` Yes, normally, and en passant |
| Does a frozen piece block sliders? | `[VERIFIED]` Yes |
| Does a frozen piece give check? | `[VERIFIED]` **No** |
| Does a frozen piece guard squares? | `[VERIFIED]` **No** |
| Can a frozen king move out of check? | `[VERIFIED]` No (canonical, `sacredRoyal` off) |
| Can I castle with a frozen rook? | `[VERIFIED]` **No** — see [`50`](50-interactions.md#castling) |
| Can I castle if the king's path is frozen? | `[VERIFIED]` **Yes** — see [`50`](50-interactions.md#castling) |
| Can I move into the zone? | `[VERIFIED]` Yes |
| How long does it last? | `[VERIFIED]` My move + the opponent's next turn |

## See also

- Cooldown and counts → [`20-spell-system.md#spell-economy`](20-spell-system.md#spell-economy)
- The jump spell → [`40-jump.md`](40-jump.md)
- Mate, stalemate, castling, en passant → [`50-interactions.md`](50-interactions.md)
- Reproducible positions → [`90-test-vectors.md`](90-test-vectors.md)

---
id: interactions
title: Spell Chess — Win Conditions, Mate Detection, and Rule Interactions
summary: How Spell Chess games end. King capture, checkmate, the spell escape hatch that suspends mate detection, stalemate, draws, castling with frozen pieces, en passant, promotion, and the threat-detection rules a bot must implement.
keywords: [spell chess, win condition, king capture, checkmate, stalemate, draw, insufficient material, castling, frozen rook, en passant, promotion, threat detection, spell escape]
answers:
  - How does a Spell Chess game end?
  - Why does the engine say it is not checkmate when I have no moves?
  - Can I castle if my rook is frozen?
  - Can I castle if the king's path is frozen?
  - What must a bot check before every move?
---

# Spell Chess — Win Conditions and Rule Interactions

> **Context:** Spell Chess is the chess.com 2-player variant. Orthodox chess plus two
> castable spells (**freeze**, **jump**). This file covers game termination and every
> cross-cutting interaction between spells and the base rules.

## Win conditions

`[VERIFIED]` A Spell Chess game is won in **two** ways, and the engine reports both with
`termination === "Checkmate"`:

### 1. Ordinary checkmate

`[VERIFIED]` The opponent is in check and has no legal escape — **and** no spell-based
escape (see [the escape hatch](#the-spell-escape-hatch)). Standard.

### 2. King capture

`[VERIFIED]` A move that captures the enemy king is legal and ends the game immediately.
Engine notation uses `xK`, e.g. `sanShort: "jump@g5 BxKh4#"`.

**Why this is reachable.** Move legality still forbids leaving *your own* king in check, so
a player can never simply walk into capture. King capture arises because **a spell cast at
the start of your turn can create an attack that did not exist during the opponent's turn**.
They had no opportunity to respond, so the attack is executed rather than answered.

The two generators of king capture:

- `[VERIFIED]` **Jump.** Make a blocking piece transparent, then slide through and take the
  king in the same turn. Fully worked example:
  [`40-jump.md#king-capture--the-signature-tactic`](40-jump.md#king-capture--the-signature-tactic).
- `[VERIFIED]` **Freeze expiry.** A player who escaped check by freezing the checker
  ([`30`](30-freeze.md#freezing-a-checker-dispels-the-check)) has only deferred it. When the
  field expires the check is live again, and if they have not resolved it the checker can
  take the king.

### Freeze mate

`[VERIFIED]` Because `sacredRoyal` is **off** in the canonical game, a frozen king cannot
move even while in check. Freeze the king's square, deliver check, and if the defender has
no other resource the game is over.

Measured: White cast `freeze@e8` and played `Rh1-h8+`. Black's king had **zero** legal
moves, and Black's only other piece had none either.

Note that this position was *not* immediately adjudicated as mate, because Black still held
usable spells — which brings us to the most important adjudication rule in the variant.

## The spell escape hatch

`[VERIFIED]` **A side with no legal moves is not checkmated or stalemated if it still holds
an unlocked spell that could generate a legal move.**

`[CODE]` Both the checkmate and stalemate predicates end with the same guard: the position
is only terminal if the player has **no** spell with `lock === 0` and `count > 0` that has
at least one legal target.

Measured outcomes for the same freeze-mate position, varying only Black's spell state:

| Black's spells | Result |
|---|---|
| 5 freeze / 2 jump, all `lock 0` | `[VERIFIED]` **not** mate — `gameOver: false` |
| 0 freeze / 0 jump | `[VERIFIED]` **Checkmate**, White wins |
| 5 freeze / 2 jump, all on cooldown | `[VERIFIED]` **Checkmate**, White wins |

`[VERIFIED]` And the escape genuinely works: from that position Black played
`freeze@h8 + a7-a6` — freezing the checking rook to dispel the check, then making an
ordinary move.

`[VERIFIED]` Stalemate obeys the same guard. A frozen king with no moves and no check, whose
owner had no other pieces, was adjudicated `"Stalemate • Draw"` — but only because no spell
could have produced a move.

**For a bot this means:** you cannot score a position as mate by looking at ordinary moves
alone. Mate detection must be:

```
isTerminal(player) :=
      no ordinary legal move
  AND not ( exists spell s : s.count > 0 AND s.lock == 0 AND s has a legal target
                             AND casting s yields >= 1 ordinary legal move )
```

`[VERIFIED]` Exactly one spell may be used for this rescue; you cannot chain two.

## Draws

| Rule | Status |
|---|---|
| Stalemate | `[VERIFIED]` draw, subject to the escape hatch above |
| Threefold repetition | `[CODE]` standard |
| Fifty-move rule | `[CODE]` standard |
| Insufficient material | `[CODE]` **suspended** while any player holds a nonzero spell count or any field is live |
| Draw by agreement | `[CODE]` supported (`playDrawAgreed`) |

`[CODE]` The insufficient-material suspension is a real rule change: K+B vs K is not
automatically drawn in a Spell Chess game where spells remain.

## Castling

`[VERIFIED]` Castling is orthodox except for these freeze interactions. Each row was
measured by freezing a single square and reading the king's destination list.

| Frozen | Effect |
|---|---|
| The **king** | `[VERIFIED]` King cannot move at all, so no castling on either side |
| The **castling rook** | `[VERIFIED]` **Castling on that side is unavailable**; the other side is unaffected |
| Squares the king **passes through or lands on** (e.g. `c1`, `d1`) | `[VERIFIED]` **No effect** — castling remains fully legal |

Measured detail: with rook `a1` frozen (zone `a1 b1 a2 b2`), the white king's destinations
lost the queenside options while retaining the kingside ones. With `c1` frozen (zone
`b1 c1 d1 b2 c2 d2`, covering the king's path and destination but neither king nor rook),
**all** castling options remained.

The logic is consistent: freeze restricts *pieces from moving*. Castling moves the rook, so a
frozen rook cannot participate. It does not move anything on the transit squares, so
freezing them is irrelevant.

`[VERIFIED]` Standard castling restrictions still apply on top: no castling out of, through,
or into check.

## En passant

`[VERIFIED]` En passant is enabled and behaves normally.

- `[VERIFIED]` A **frozen pawn can be captured en passant.** Measured: Black froze a zone
  containing its own pawn on `c5` but not White's pawn on `d5`; White's `dxc6` en passant
  was legal.
- `[VERIFIED]` There is **no special en-passant freeze rule.** A tempting false conclusion
  is that freezing the victim cancels en passant — it does not. What actually happens is
  that any 3×3 zone centred on or near the victim pawn usually also covers the capturing
  pawn (they are adjacent), freezing *your own* pawn and thereby removing the capture. Check
  the zone geometry before concluding anything here.

## Promotion

`[VERIFIED]` Promotion is orthodox: reach the last rank, choose Q/R/B/N. Engine emits e.g.
`b8=Q+`. A pawn on a frozen square cannot move and therefore cannot promote.

## Threat detection for bots

`[VERIFIED]` A Spell Chess engine that only asks "am I in check?" will lose to spell tactics.
Before committing to a move, evaluate whether the resulting position lets the opponent win
**in a single turn**. The opponent's one-turn resources are:

1. **Jump → king capture.** For every enemy slider (Q/R/B), test whether it would attack your
   king if exactly one occupied square on the ray between them were made transparent. If so,
   and the opponent has `jump.count > 0 && jump.lock === 0`, they win on the spot.
2. **Jump → unblockable check.** Same construction, but where the resulting check merely
   cannot be blocked. Your king must have a flight square, or you must be able to capture the
   checker. See [`40`](40-jump.md#checks-through-a-jump-square-are-unblockable).
3. **Freeze → mate.** If your king's only escape squares belong to a single 3×3 block
   together with your king, a freeze may immobilise it while a check is delivered.
4. **Freeze → defence removal.** Freezing a defender removes all of its control
   ([`30`](30-freeze.md#frozen-pieces-exert-no-control)), so a piece it was guarding becomes
   free to take, and a pin it was maintaining evaporates.
5. **Freeze → castling denial.** Freezing your rook removes that side's castling for a turn.

Symmetrically, your own one-turn winning resources are the same list with colours reversed.

`[VERIFIED]` Also track the mirror of rule 2 for yourself: **a jump field you create is
usable by your opponent on their very next turn** ([`40`](40-jump.md#who-can-use-the-field)).
Always search the opponent's replies with your own field still on the board.

## Quick reference: what changes vs. standard chess

| | Standard chess | Spell Chess |
|---|---|---|
| Turn | 1 move | `[VERIFIED]` optional spell + 1 move |
| Passing | never | `[VERIFIED]` never |
| King capture | impossible | `[VERIFIED]` legal, wins |
| Ignoring check | impossible | `[VERIFIED]` impossible — but freezing the checker removes it |
| Mate detection | no legal moves + check | `[VERIFIED]` also requires no spell rescue |
| Insufficient material | draws | `[CODE]` suspended while spells remain |
| Castling | K/R unmoved, path clear & safe | `[VERIFIED]` plus: rook must not be frozen |
| Blocking a check | always possible on the ray | `[VERIFIED]` impossible through a jump square |

## See also

- [`30-freeze.md`](30-freeze.md) · [`40-jump.md`](40-jump.md) · [`20-spell-system.md`](20-spell-system.md)
- Reproducible positions → [`90-test-vectors.md`](90-test-vectors.md)

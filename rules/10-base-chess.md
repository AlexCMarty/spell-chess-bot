---
id: base-chess
title: Spell Chess — Which Orthodox Chess Rules Apply
summary: The orthodox chess baseline for Spell Chess. Everything standard unless listed as modified. Covers movement, castling, en passant, promotion, check legality, and the draw rules.
keywords: [spell chess, base rules, orthodox chess, castling, en passant, promotion, check, legal moves, fifty move rule, threefold repetition]
answers:
  - Do normal chess rules apply in Spell Chess?
  - Is en passant legal? Is castling legal? Is promotion normal?
  - Can I move into check in Spell Chess?
  - Which standard rules are changed or disabled?
---

# Spell Chess — Orthodox Chess Baseline

> **Context:** Spell Chess is the chess.com 2-player variant. Orthodox chess plus two
> castable spells (**freeze**, **jump**). This file states which standard chess rules carry
> over unchanged, so that the spell files only have to describe deltas.

## Default: everything is standard chess

`[VERIFIED]` Start position, piece movement, capture, turn alternation, castling, en
passant, promotion and the draw rules are all orthodox. Spell Chess adds mechanics; it does
not rewrite the base game. If a situation involves no live spell field and no spell being
cast, **it resolves exactly as in standard chess**.

## Unchanged rules

| Rule | Status | Note |
|---|---|---|
| Starting position | `[VERIFIED]` standard | Verified byte-identical to a programmatically built standard array |
| Piece movement and capture | `[VERIFIED]` standard | |
| Pawn double-step from rank 2 / 7 | `[VERIFIED]` standard | But see [`40-jump.md#pawn-double-step`](40-jump.md#pawn-double-step) |
| En passant | `[VERIFIED]` standard, **enabled** | `ruleVariants.enPassant === true` |
| Castling, both sides | `[VERIFIED]` standard | Modified by freeze — see [`50-interactions.md#castling`](50-interactions.md#castling) |
| Castling out of / through / into check | `[VERIFIED]` forbidden, as standard | |
| Promotion on rank 8 / 1, to Q R B N | `[VERIFIED]` standard | Engine emits e.g. `b8=Q+` |
| Threefold repetition | `[CODE]` standard | Position hashing includes spell state |
| Fifty-move rule | `[CODE]` standard | `pliesTill50MoveRule()` |
| Stalemate is a draw | `[VERIFIED]` standard | Termination string `"Stalemate • Draw"` |

## Modified rules

### You still may not leave your own king in check — unless the move captures the enemy king

`[VERIFIED]` Move legality is enforced normally: a move that leaves your king attacked is
rejected. There is **no** general "you may ignore check" rule.

The apparent counter-example — a player legally ignoring a check — is usually the freeze
spell removing the check first. A frozen piece gives no check, so after freezing the
checker there is simply no check to answer. See
[`30-freeze.md#freezing-a-checker-dispels-the-check`](30-freeze.md#freezing-a-checker-dispels-the-check).

`[VERIFIED]` The other, genuine exception: a move that **captures the enemy king** is legal
even if it leaves the mover's own king in check — including an unrelated pre-existing
check, and even a pre-existing *double* check. The one thing it may not do is **newly
create** a second, simultaneous check via this exact move (e.g. a pinned piece breaking its
pin to reach the enemy king) when the mover wasn't already facing that many attackers —
that specific escalation is still illegal, mirroring the orthodox rule that only a king
move answers double check. Being already in a double check you didn't just cause doesn't
block the capture; causing one yourself does. See
[`50-interactions.md#win-conditions`](50-interactions.md#win-conditions) for the three
measured examples that pin this down.

### The king can be captured

`[VERIFIED]` Unlike standard chess, a move that captures the enemy king is legal and ends
the game. This is reachable because a spell cast at the start of your turn can create an
attack on the enemy king that did not exist when they moved — so they never had a chance to
answer it. Full treatment: [`50-interactions.md#win-conditions`](50-interactions.md#win-conditions).

### Insufficient material is disabled while spells remain

`[CODE]` The insufficient-material draw check returns "not a draw" whenever any player
still holds a nonzero spell count **or** any spell field is currently on the board. K+B vs
K is therefore **not** an automatic draw early in a Spell Chess game.

`[CODE]` Once both players have spent every freeze and every jump and no field is live, the
standard insufficient-material logic resumes.

### Checkmate and stalemate detection is spell-aware

`[VERIFIED]` A side with no legal moves is **not** mated or stalemated if it holds an
unlocked spell that could generate a legal move. This is a genuine rule change with real
consequences for search. See
[`50-interactions.md#the-spell-escape-hatch`](50-interactions.md#the-spell-escape-hatch).

## Rules that do NOT apply

`[VERIFIED]` The engine supports many other variants through the same code path. All of the
following are **off** in canonical Spell Chess and must not be implemented:

`atomic`, `giveaway`, `crazyhouse`, `duckChess`, `fogOfWar`, `koth` (king of the hill),
`captureTheKing` (the dedicated variant flag — note the king is still capturable in Spell
Chess, but by the mechanism described above, not by this flag), `nCheck`, `torpedo`,
`taboo`, `chess960`, `seirawanSetup`, `barePieceLoses`, `allowPassing`.

`[VERIFIED]` In particular `allowPassing` is **false**: you can never pass. Every turn must
contain exactly one piece move.

## Time control

`[VERIFIED]` The default rated queue is **5|2 blitz** (5 minutes, 2-second increment).
Not a rule of the variant, but relevant to bot time management.

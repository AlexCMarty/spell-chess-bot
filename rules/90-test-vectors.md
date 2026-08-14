---
id: test-vectors
title: Spell Chess — Verified Test Vectors
summary: Concrete positions with engine-verified expected output, for regression-testing a Spell Chess move generator. Every vector was produced by executing chess.com's own engine.
keywords: [spell chess, test vectors, regression tests, expected output, move generation, validation, ground truth, perft, positions]
answers:
  - What positions should I test my Spell Chess engine against?
  - What is the expected legal move list for position X?
  - How do I know my move generator is correct?
---

# Spell Chess — Verified Test Vectors

> **Context:** Spell Chess is the chess.com 2-player variant. Orthodox chess plus two
> castable spells (**freeze**, **jump**). Every vector below is real output from chess.com's
> engine, captured on 2026-08-09 against bundle `variants/2026.8.1`. Squares are **8×8**
> notation. Reproduce any of them with the harness in
> [`70-engine-api.md`](70-engine-api.md).

## Notation used here

- **Pieces** — `{e1:'0K', e8:'2K'}` where `0` = White, `2` = Black.
- **Turn** — seat to move, `0` or `2`.
- **Spells** — default is 5 freeze / 2 jump, no cooldown, unless stated.
- **A turn** is written `spell + move`, e.g. `freeze@d5 + a1a2`.
- Unless noted, all castling rights are **off** (so king destination lists show only
  ordinary king moves).

---

## 1 — Start position sanity

**Position:** standard start, White to move.

```
e2 destinations   →  [e3, e4]
g1 destinations   →  [h3, f3]
freeze targets    →  all 64 squares
jump targets      →  the 32 occupied squares only
```

**Asserts:** freeze may target empty squares; jump may not.

---

## 2 — Freeze zone, centre of board

**Position:** `{e1:'0K', a1:'0R', e8:'2K', d5:'2R', h7:'2N'}`, White to move.
**Turn played:** `freeze@d5 + a1a2` → SAN `freeze@g8&Rd4-d5`

Black to move; zone is `c4 c5 c6 d4 d5 d6 e4 e5 e6`.

```
d5 (black rook, frozen)  →  []
e8 (black king)          →  [d7, d8, e7, f7, f8]
h7 (black knight)        →  [f8, f6, g5]
spellFields              →  [{target:'d5', player:0, type:'freeze', life:1}]
```

**Asserts:** the frozen piece is immobilised; pieces outside the zone are unaffected.

---

## 3 — Freeze zone clipping at a corner

**Position:** any; White casts `freeze@a1`.

```
isFrozen(a1) → true      isFrozen(a2) → true
isFrozen(b2) → true      isFrozen(c3) → false
```

**Asserts:** a corner freeze covers 2×2 = `{a1, a2, b1, b2}`, not 3×3.

---

## 4 — A frozen piece exerts no control

**Position:** `{e1:'0K', h4:'0R', d5:'2K', a8:'2R'}`, Black to move.

Baseline, no spell — the white rook on `h4` guards rank 4:

```
d5 (black king)  →  [c5, c6, d6, e5, e6]
```

**Turn played:** `freeze@h4 + a8a7`, then re-query Black's king:

```
d5 (black king)  →  [c4, c5, c6, d4, d6, e4, e5, e6]
```

**Asserts:** `c4`, `d4`, `e4` become legal. A frozen piece guards nothing, so the enemy king
may walk into its line. **This is the highest-value vector in the suite** — a naive
implementation that only blocks frozen pieces from *moving* fails it.

---

## 5 — Freezing the checker dispels the check

**Position:** `{e1:'0K', d8:'0R', a1:'0R', e8:'2K', h8:'2R'}`, Black to move.
Black is in check from `Rd8`.

Baseline legal moves for Black: `e8 → [d8, e7, f7]`, `h8 → []`.

**Turn played:** `freeze@d8 + h8h7` → SAN `freeze@d8 Rh7` — **legal**.

Then, White to move:

```
d8 (white rook, frozen)  →  []
a1 (white rook)          →  [b1, c1, d1, a2, a3, a4, a5, a6, a7, a8]
```

**Asserts:** Black legally ignored the check because the checker was frozen; and the frozen
rook cannot capture the king on the following turn either.

---

## 6 — A frozen piece still blocks and can still be captured

**Position:** `{e1:'0K', d1:'0R', e8:'2K', d5:'2N', d8:'2R'}`, Black to move.
**Turn played:** `freeze@d5 + d8d7` (Black freezes its own knight).

White to move:

```
d1 (white rook)  →  [c1, b1, a1, d2, d3, d4, d5]
d1→d5 capture    →  legal, SAN "Rxd5"
```

**Asserts:** the frozen knight still blocks the file beyond `d5`, and is capturable.

---

## 7 — Your own freeze binds your own pieces, same turn

**Position:** `{e1:'0K', a1:'0R', d4:'0N', e8:'2K', h8:'2R'}`, White to move.
Baseline: `d4 → [f5, e6, c6, b5, b3, c2, e2, f3]`.

```
freeze@d4 + d4c6   →  REJECTED   (the knight it just froze cannot move)
freeze@d4 + a1a2   →  legal
```

**Asserts:** the spell resolves before your move and constrains it.

---

## 8 — But you may move a different piece *into* your own zone

**Position:** `{e1:'0K', a1:'0R', f5:'0N', e8:'2K', h8:'2R'}`, White to move.

```
freeze@d5 + f5d6   →  legal, SAN "freeze@d5 Nd6"
```

**Asserts:** the restriction is on the **origin** square only.

---

## 9 — King capture via jump  ★ the signature tactic

**Position:** `{e1:'0K', d2:'0B', a1:'0R', h2:'0R', e8:'2K', b4:'2B', a8:'2R'}`, Black to move.

Baseline: White is **not** in check; `b4 → [a3, a5, c3, d2, c5, d6, e7, f8]` (blocked by `Bd2`).

**Turn played:** `jump@d2 + b4e1`

```
san          →  "jump@g5&Be7xKh4#"
san8x8       →  "jump@d2 Bxe1#"
sanShort     →  "jump@g5 BxKh4#"
gameOver     →  true
termination  →  "Checkmate"
winner       →  2   (Black)
```

**Asserts:** king capture is legal, is reachable in one turn from a safe-looking position,
and is scored as a checkmate win. This vector reproduces a real chess.com game verbatim.

---

## 10 — A jump field serves both players

**Position:** `{e1:'0K', d1:'0R', d4:'0P', e8:'2K', d8:'2R', a1:'0R'}`, White to move.
Baseline: `d1 (white rook) → [c1, b1, d2, d3]` (blocked by its own pawn on `d4`).

**Turn played:** `jump@d4 + a1a2` — White jumps its **own** pawn.

```
d8 (BLACK rook)  →  [c8, b8, a8, d7, d6, d5, d4, d3, d2, d1]
d1 (white rook)  →  [c1, b1, a1, d2, d3, d5, d6, d7, d8]
```

**Asserts:** Black's rook sees straight through White's pawn. The field is a property of the
square, not of the caster.

---

## 11 — Pawn double-step over a jumped blocker

**Position:** `{e1:'0K', e8:'2K', d2:'0P', d3:'2R', a1:'0R', h8:'2R'}`, White to move.
Baseline: `d2 (white pawn) → []` — completely blocked.

**Turn played:** `jump@d3 + a1a2`, then re-query the pawn:

```
d2 (white pawn)  →  [d4]
```

**Asserts:** the double-step hops the transparent square; `d3` itself remains unreachable
because pawns do not capture forward.

---

## 12 — A check through a jump square cannot be blocked on the jump square

**Position:** as vector 9, Black to move.
**Turn played:** `jump@d2 + a8a7` → SAN `jump@g5&Rd11-d10+` (the jump reveals `Bb4`'s check).

White to move, in check:

```
e1 (white king)    →  [d1, e2, f1, f2]
d2 (white bishop)  →  [c3, b4]           ← c3 interposes; b4 captures the checker
a1 (white rook)    →  []                 ← cannot reach c3 or d2
```

**Asserts:** landing **on** the jump square (`d2`) does not block. Interposition on other
between-squares is legal (`Bd2-c3`). The rook on `a1` has zero moves because it cannot
reach `c3`/`d2`, not because every interposition is illegal.

---

## 13 — Freeze mate, and the spell escape hatch

**Position:** `{e1:'0K', a1:'0R', h1:'0R', e8:'2K', a7:'2P'}`, White to move.
**Turn played:** `freeze@e8 + h1h8` → SAN `freeze@e8 Rh8+`

Black to move, in check, with **no ordinary legal moves**:

```
e8 (black king, frozen)  →  []
a7 (black pawn)          →  []
inCheck                  →  true
```

Adjudication depends **entirely** on Black's spell state:

| Black's spells | `gameOver` | `termination` |
|---|---|---|
| 5 freeze / 2 jump, `lock 0` | `false` | — (not mate) |
| `jump_0x0, freeze_0x0` (exhausted) | `true` | `"Checkmate"`, winner 0 |
| `jump_3x2, freeze_3x5` (on cooldown) | `true` | `"Checkmate"` |

And the escape genuinely works from the first row:

```
freeze@h8 + a7a6   →  legal, SAN "freeze@h8 a6"
```

**Asserts:** mate detection must account for a one-spell rescue; exhausted **and** locked
spells both fail to rescue.

---

## 14 — Stalemate is also spell-aware

**Position:** `{e1:'0K', a1:'0R', b1:'0R', e8:'2K'}`, White to move.
**Turn played:** `freeze@e8 + a1a2`

```
e8 (black king, frozen)  →  []
gameOver                 →  true
termination              →  "Stalemate • Draw"
```

**Asserts:** with Black holding only a king, no spell can create a move, so stalemate stands.

---

## 15 — Castling under freeze

**Position:** `{e1:'0K', h1:'0R', a1:'0R', e8:'2K', h8:'2R', a8:'2R'}`, all castling rights on.

Baseline White king destinations (`g1`/`c1` = castling; `h1`/`a1` = the rook-square form):

```
e1  →  [d1, d2, e2, f1, f2, h1, g1, a1, c1]
```

Black casts a freeze, then White's king destinations are re-queried:

| Black's cast | Zone covers | White `e1` destinations | Effect |
|---|---|---|---|
| `freeze@c1` | king's path `d1` + target `c1` | `[d1, d2, e2, f1, f2, h1, g1, a1, c1]` | **no effect** |
| `freeze@a1` | rook `a1`, `b1` | `[d1, d2, e2, f1, f2, h1, g1]` | queenside lost |
| `freeze@b2` | rook `a1` | `[d1, d2, e2, f1, f2, h1, g1]` | queenside lost |
| `freeze@g1` | rook `h1` | `[d1, d2, e2, f1, f2, a1, c1]` | kingside lost |
| `freeze@e1` | the **king** | `[]` | king frozen, nothing legal |

**Asserts:** freezing the castling **rook** removes that side's castling; freezing the
squares the king **travels through or lands on** does not.

---

## 16 — En passant against a frozen pawn

**Position:** `{e1:'0K', e8:'2K', d5:'0P', a1:'0R', h8:'2R', c7:'2P'}`, Black to move.
**Turn played:** `freeze@b4 + c7c5` — the zone `a3 b3 c3 a4 b4 c4 a5 b5 c5` covers the black
pawn's landing square `c5` but **not** White's pawn on `d5`.

White to move:

```
isFrozen(c5)     →  true
isFrozen(d5)     →  false
d5 (white pawn)  →  [d6, c6]
d5→c6            →  legal, SAN "dxc6"   (en passant capture of a frozen pawn)
```

**Asserts:** there is no special en-passant freeze rule. A frozen pawn is capturable en
passant. Beware: most zones centred near the victim also freeze the capturing pawn, which is
what makes this look like a special rule.

---

## 17 — Promotion

**Position:** `{e1:'0K', e8:'2K', b7:'0P', a1:'0R', h8:'2R'}`, White to move.

```
b7 destinations   →  [b8]
b7→b8 promote Q   →  legal, SAN "b8=Q+"
```

**Asserts:** promotion is orthodox.

---

## 18 — Cooldown timeline

**Position:** `{e1:'0K', a1:'0R', e8:'2K', a8:'2R'}`, White to move.
**Turn played:** `freeze@d5 + a1a2`, then quiet moves by both sides.

| After | White `freeze` | Field on board |
|---|---|---|
| White's casting turn *N* | `{count:4, lock:3}` | yes |
| Black's turn *N* | `{count:4, lock:2}` | **no** (expired) |
| White's turn *N+1* | `{count:4, lock:2}` | no |
| Black's turn *N+1* | `{count:4, lock:1}` | no |
| White's turn *N+2* | `{count:4, lock:1}` | no |
| Black's turn *N+2* | `{count:4, lock:0}` | no |
| White's turn *N+3* | castable again | — |

Recasting on turn *N+3* succeeded (`freeze@e5 Ra3`) and produced `{count:3, lock:3}`.

**Asserts:** cooldown is 3 of your own turns; the field expires after one opponent turn;
the count decrements 5 → 4 → 3 and **never** returns.

---

## 19 — Illegal casts are rejected

**Position:** any, with White's spells set to `jump_0x0,freeze_2x5`.

```
jump@<any occupied square> + a1a2   →  REJECTED   (count === 0)
freeze@<any square>        + a1a2   →  REJECTED   (lock === 2)
```

**Asserts:** a cast requires `count > 0 && lock === 0`.

---

## 20 — API permissiveness (do NOT treat as legal play)

`[VERIFIED]` The raw engine accepts both of these; **neither is legal Spell Chess**:

```
play({multimoves:[freeze@d5, jump@a8, a1a2]})  →  accepted, SAN "freeze@d5 jump@a8 Ra2"
play({from:'@@0_freeze', to:…})   alone        →  accepted, turn advances
```

The one-spell-per-turn limit and the mandatory piece move are enforced by the client UI
(the turn is modelled as exactly 2 phases), not by `play()`.

**Asserts:** your bot must impose the 2-phase structure itself. See
[`20-spell-system.md#turn-structure`](20-spell-system.md#turn-structure).

---

## Suggested test order

1. Vectors 1, 2, 3 — basic freeze geometry.
2. **Vector 4** — no-control rule. Most implementations fail here first.
3. Vectors 5, 6, 7, 8 — freeze semantics.
4. Vectors 10, 11, 12 — jump semantics.
5. **Vector 9** — king capture. If this fails, the bot cannot win or defend properly.
6. Vectors 13, 14 — mate/stalemate adjudication with the escape hatch.
7. Vectors 15, 16, 17 — castling, en passant, promotion.
8. Vectors 18, 19 — spell economy.

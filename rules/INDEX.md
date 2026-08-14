---
id: index
title: Spell Chess Rules — Retrieval Index
summary: Routing table for the Spell Chess rule corpus. Maps questions to the exact file and anchor that answers them. Read this first.
keywords: [spell chess, index, routing, table of contents, retrieval, chess.com variant]
answers:
  - Where do I find rule X for Spell Chess?
  - Which file covers freeze / jump / win conditions / notation?
audience: AI coding agent building a Spell Chess bot
---

# Spell Chess Rules — Retrieval Index

> **Context header (repeated in every chunk of this corpus):** Spell Chess is the
> chess.com 2-player variant at `https://www.chess.com/variants/spell-chess`. Orthodox
> chess plus two castable spells (**freeze**, **jump**). This corpus is the ground-truth
> rule reference, derived from and verified against chess.com's own client engine.

## How to use this corpus

1. **Always start here.** Find your question in the routing table below.
2. **Read only the file(s) you need.** Every file is self-contained; no file requires
   another to be understood.
3. **Trust the confidence tags.** Every rule statement carries one of:
   - `[VERIFIED]` — proven by executing the real chess.com engine and observing output.
   - `[CODE]` — read directly from the engine source (`variants.js`), not separately executed.
   - `[DOC]` — from chess.com's own prose (help centre / in-client queue description).
   - `[UNVERIFIED]` — inference. Treat as a hypothesis; confirm before relying on it.
4. **When code and prose disagree, code wins.** chess.com's public help pages contain
   at least three outright errors (see `00-overview.md#known-errors-in-public-sources`).

## Routing table

| If you need to know… | Go to |
|---|---|
| What the game is, the canonical config, board/coordinate model | [`00-overview.md`](00-overview.md) |
| Which orthodox chess rules still apply, and which don't | [`10-base-chess.md`](10-base-chess.md) |
| Turn structure: when you cast, whether a move is mandatory | [`20-spell-system.md#turn-structure`](20-spell-system.md#turn-structure) |
| Spell counts, cooldowns, when a spell becomes available again | [`20-spell-system.md#spell-economy`](20-spell-system.md#spell-economy) |
| How long a cast spell stays on the board | [`20-spell-system.md#field-lifetime`](20-spell-system.md#field-lifetime) |
| Freeze: legal targets, 3×3 geometry, edge clipping | [`30-freeze.md#targeting-and-geometry`](30-freeze.md#targeting-and-geometry) |
| Freeze: what a frozen piece can and cannot do | [`30-freeze.md#effects-on-a-frozen-piece`](30-freeze.md#effects-on-a-frozen-piece) |
| Freeze: does a frozen piece give check / guard squares? | [`30-freeze.md#frozen-pieces-exert-no-control`](30-freeze.md#frozen-pieces-exert-no-control) |
| Escaping check by freezing the checker | [`30-freeze.md#freezing-a-checker-dispels-the-check`](30-freeze.md#freezing-a-checker-dispels-the-check) |
| Jump: legal targets, what transparency means | [`40-jump.md#targeting`](40-jump.md#targeting) |
| Jump: which pieces benefit, whether the opponent benefits too | [`40-jump.md#who-can-use-the-field`](40-jump.md#who-can-use-the-field) |
| Jump: pawn double-step, jump-through-check interposition | [`40-jump.md#pawn-double-step`](40-jump.md#pawn-double-step) |
| **How the king gets captured / how to win** | [`50-interactions.md#win-conditions`](50-interactions.md#win-conditions) |
| Why checkmate is sometimes *not* checkmate | [`50-interactions.md#the-spell-escape-hatch`](50-interactions.md#the-spell-escape-hatch) |
| Castling with a frozen king or frozen rook | [`50-interactions.md#castling`](50-interactions.md#castling) |
| En passant + spells, promotion + spells | [`50-interactions.md#en-passant`](50-interactions.md#en-passant) |
| Draws, insufficient material, 50-move, repetition | [`50-interactions.md#draws`](50-interactions.md#draws) |
| SAN / PGN / FEN encoding of spells | [`60-notation-and-encoding.md`](60-notation-and-encoding.md) |
| Driving the real engine from JS to test a bot | [`70-engine-api.md`](70-engine-api.md) |
| Concrete positions + expected legal moves (regression suite) | [`90-test-vectors.md`](90-test-vectors.md) |

## The 12 rules that most often surprise an implementer

Read these before writing any move generator. Each links to its full treatment.

1. A turn is **[optional spell] + [exactly one piece move]**. The move is mandatory; a
   spell alone is not a turn. → [`20`](20-spell-system.md#turn-structure)
2. **At most one spell per turn.** → [`20`](20-spell-system.md#turn-structure)
3. Spell counts (5 freeze, 2 jump) **never replenish**. "Recharge" refers only to the
   cooldown. → [`20`](20-spell-system.md#spell-economy)
4. A freeze zone freezes **your own pieces too**, and applies **during your own move**.
   → [`30`](30-freeze.md#the-zone-affects-the-caster-too)
5. A frozen piece **exerts no control at all** — no check, no guarded squares. The enemy
   king may walk into its line. → [`30`](30-freeze.md#frozen-pieces-exert-no-control)
6. Freezing the piece that checks you **dispels the check**; you may then ignore it
   entirely and play any legal move. → [`30`](30-freeze.md#freezing-a-checker-dispels-the-check)
7. A frozen piece **still blocks sliders and can still be captured**.
   → [`30`](30-freeze.md#a-frozen-piece-still-occupies-its-square)
8. A jump field belongs to the **square**, not the piece, and is usable by **both
   players**. → [`40`](40-jump.md#who-can-use-the-field)
9. A check delivered through a jump square **cannot be blocked by landing on the
   jump square**; other between-squares still allow interposition.
   → [`40`](40-jump.md#checks-through-a-jump-square-are-unblockable)
10. **The king can be captured, and it wins the game.** This is the variant's signature
    tactic, not an edge case. → [`50`](50-interactions.md#win-conditions)
11. **Checkmate is not declared while the mated side still has an unlocked spell** that
    could produce a legal move. → [`50`](50-interactions.md#the-spell-escape-hatch)
12. Freezing the **castling rook** removes castling on that side; freezing the squares
    the king travels through does **not**. → [`50`](50-interactions.md#castling)

## Provenance

Derived from chess.com client bundle
`https://www.chess.com/r2/client-packages/variants/2026.8.1/variants.js`, and verified by
executing that engine in-browser on the analysis board. See
[`70-engine-api.md`](70-engine-api.md) to reproduce any claim. Corpus written 2026-08-09.

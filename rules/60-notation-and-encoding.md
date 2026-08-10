---
id: notation-encoding
title: Spell Chess — SAN, PGN and FEN Encoding
summary: How spells appear in move notation, PGN headers and move text, and how spell counts, cooldowns and live fields are encoded in the engine's extended FEN.
keywords: [spell chess, notation, SAN, PGN, FEN, san8x8, spell encoding, spellFields, RuleVariants, king capture notation, parsing]
answers:
  - How is a spell written in SAN or PGN?
  - How do I parse a Spell Chess PGN?
  - Where are spell counts stored in the FEN?
  - What does jump_0x2,freeze_0x5 mean?
  - How is a king capture notated?
---

# Spell Chess — Notation and Encoding

> **Context:** Spell Chess is the chess.com 2-player variant. Orthodox chess plus two
> castable spells (**freeze**, **jump**). This file specifies every serialisation format a
> bot must read or write.

## Spell move notation

`[VERIFIED]` A cast is written `<type>@<square>`:

```
freeze@d5      jump@d2
```

`[VERIFIED]` A turn that includes a spell joins the two phases. **The separator differs by
field**, which is a common parsing trap:

| Field | Coordinates | Separator | Example |
|---|---|---|---|
| `san` | 14×14 | `&` | `jump@g5&Be7xKh4#` |
| `san8x8` | 8×8 | space | `jump@d2 Bxe1#` |
| `sanShort` | 14×14 | space | `jump@g5 BxKh4#` |

`[VERIFIED]` `san8x8` is the human-facing form and the one to use for a 2-player bot. See
[`00-overview.md#coordinate-model`](00-overview.md#coordinate-model) for the mapping.

### King capture notation

`[VERIFIED]` A move capturing the king is marked with an explicit `K` after the capture
sign, and terminated with `#`:

```
sanShort : "jump@g5 BxKh4#"      ← xK = king captured
san8x8   : "jump@d2 Bxe1#"       ← 8x8 form renders as an ordinary capture
```

`[VERIFIED]` Ordinary suffixes are standard: `+` check, `#` mate/king-capture, `=Q`
promotion (e.g. `b8=Q+`).

## PGN

`[VERIFIED]` The variant is identified by the `RuleVariants` header:

```
[Variant "..."]
[RuleVariants "EnPassant Play4Mate Spell=1+3"]
[CurrentMove "0"]
[TimeControl "5|2"]
```

`[CODE]` The `Spell` token is matched by `/Spell=?([1-9][0-9]*\+[1-9][0-9]*!?)?/i`.
`Spell` with no `=value` means the default. For the parameter's meaning see
[`00-overview.md#reading-the-spell-parameter`](00-overview.md#reading-the-spell-parameter).

### Move text

`[VERIFIED]` chess.com's PGN for 2-player games uses the 4-player move format: seats 1 and 3
are written as `..` placeholders. A move number covers a full cycle of all four seats.

```
1. g5-g7 .. h10-h8
2. h5-h7 .. jump@g10&Qg11xg7
3. k5-k6 .. Bi11-e7+
4. Bf4-g5 .. Be7xBg5+ ( .. Kh11-h10
5. Rk4-k5 .. jump@g5&Be7xKh4# )
```

Reading that:

- `g5-g7` is White; the `..` is the dead seat 1; `h10-h8` is Black.
- Coordinates are **14×14**. Subtract 3 from file and rank: `g5-g7` = `d2-d4`.
- `jump@g10&Qg11xg7` is one Black turn: cast `jump@d7`, then `Qd8xd4`.
- `( … )` delimits a variation, exactly as in standard PGN.
- `Be7xBg5+` includes the captured piece's letter (`xB`), a 4-player-PGN convention.

`[VERIFIED]` The same game in 8×8 terms is `1. d4 e5 2. e4 (jump@d7 Qxd4) 3. h3 Bb4+
4. Bd2 …`.

## FEN

`[VERIFIED]` The engine uses an extended, 8-field, hyphen-separated FEN over a 14×14 grid.

```
<board>-<turn>-<dead>-<castleKS>-<castleQS>-<points>-<plies>-<flexdata JSON>
```

Canonical Spell Chess start position:

```
x,x,x,x,x,x,x,x,x,x,x,x,x,x/ … /x,x,x,2R,2N,2B,2Q,2K,2B,2N,2R,x,x,x/ …
-0
-[false,true,false,true]
-[true,true,true,true]
-[true,true,true,true]
-[0,0,0,0]
-0
-{"pawnBaseRank":5,"wb":true,"dim":"8x8","spells":["jump_0x2,freeze_0x5","","jump_0x2,freeze_0x5",""],"spellFields":[]}
```

| Field | Meaning |
|---|---|
| 1 `board` | 14 rows, top (rank 14) first, comma-separated cells. `x` = off-board. `<n>` = n empty squares. `<player><piece>` = a piece, e.g. `0K` white king, `2Q` black queen. |
| 2 `turn` | Seat to move: `0` White, `2` Black |
| 3 `dead` | Per-seat dead flags. 2-player: `[false,true,false,true]` |
| 4 `castleKS` | Per-seat kingside castling rights |
| 5 `castleQS` | Per-seat queenside castling rights |
| 6 `points` | Per-seat score (unused in 2-player) |
| 7 `plies` | Halfmove counter for the fifty-move rule |
| 8 `flexdata` | JSON blob, containing the spell state |

### FEN spell encoding

`[VERIFIED]` Inside `flexdata`:

```json
"spells": ["jump_0x2,freeze_0x5", "", "jump_0x2,freeze_0x5", ""]
```

- One entry per seat, indices 0–3. Seats 1 and 3 are empty strings in a 2-player game.
- Each entry is a comma-separated list of `<type>_<lock>x<count>`.
- **`jump_0x2` = jump, lock 0, count 2.** The lock comes first.

```
freeze_0x5   →  5 freezes available, no cooldown
freeze_3x4   →  4 freezes left, 3 turns of cooldown remaining
jump_0x0     →  jump exhausted
```

`[VERIFIED]` Live fields:

```json
"spellFields": []
```

`[VERIFIED]` At runtime each field is the object
`{ target, player, type, life }`, e.g.

```json
{ "target": "g8", "player": 0, "type": "freeze", "life": 1 }
```

where `target` is a **14×14** square. `[CODE]` The serialised string form used elsewhere in
the client is `` `${life}_${player}_${type}@${target}` ``, e.g. `1_0_freeze@g8`.

## Parsing checklist

When writing a parser, get these five things right:

1. `[VERIFIED]` Convert coordinates: 14×14 ↔ 8×8 is ±3 on both file and rank.
2. `[VERIFIED]` Seats are 0 and 2, never 1 and 3.
3. `[VERIFIED]` `san` uses `&` between spell and move; `san8x8` uses a space.
4. `[VERIFIED]` `<type>_<lock>x<count>` — lock first, count second.
5. `[VERIFIED]` A PGN move number spans four seats, two of which are `..`.

## See also

- Coordinate model → [`00-overview.md#coordinate-model`](00-overview.md#coordinate-model)
- Driving the engine → [`70-engine-api.md`](70-engine-api.md)

#![no_main]

use libfuzzer_sys::fuzz_target;
use spellchess_core::*;

/// Reads pseudo-random choices from the fuzzer's raw bytes, returning 0 once
/// exhausted so short/mutated inputs still decode to *some* (boring) position
/// instead of failing -- libFuzzer's own coverage-guided mutation is what
/// grows more interesting inputs from there, not this reader.
struct ByteReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> ByteReader<'a> {
    fn new(data: &'a [u8]) -> ByteReader<'a> {
        ByteReader { data, pos: 0 }
    }

    fn next_u8(&mut self) -> u8 {
        let b = self.data.get(self.pos).copied().unwrap_or(0);
        self.pos += 1;
        b
    }

    fn range(&mut self, n: usize) -> usize {
        self.next_u8() as usize % n
    }
}

const KINDS: [PieceKind; 5] =
    [PieceKind::Pawn, PieceKind::Knight, PieceKind::Bishop, PieceKind::Rook, PieceKind::Queen];

fn adjacent(a: Square, b: Square) -> bool {
    a != b && a.file().abs_diff(b.file()) <= 1 && a.rank().abs_diff(b.rank()) <= 1
}

fn take(r: &mut ByteReader, free: &mut Vec<Square>) -> Square {
    let idx = r.range(free.len());
    free.remove(idx)
}

fn place(r: &mut ByteReader, free: &mut Vec<Square>, pos: &mut Position, sq: Square) {
    free.retain(|&s| s != sq);
    let kind = KINDS[r.range(KINDS.len())];
    let color = if r.next_u8() & 1 == 0 { Color::White } else { Color::Black };
    if kind == PieceKind::Pawn && (sq.rank() == 0 || sq.rank() == 7) {
        return;
    }
    pos.board.set(sq, Some(Piece { color, kind }));
}

/// Byte-driven counterpart of `king_tangle_positions` in
/// `crates/core/tests/spell_delta_soundness.rs` -- same construction (kings
/// placed, 1-3 pieces crowding them, 1-4 more anywhere, up to 2 spell fields
/// half-anchored on a king), sourced from fuzzer bytes instead of a
/// splitmix64 seed so libFuzzer's coverage-guided mutation can steer it.
/// Duplicated rather than shared, matching the existing precedent of
/// `spell_delta.rs`'s own `mod tests` keeping its own copy of `rescan_oracle`.
fn decode_position(r: &mut ByteReader) -> Position {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.castle_rights = CastleRights {
        white_kingside: false,
        white_queenside: false,
        black_kingside: false,
        black_queenside: false,
    };
    let mut free: Vec<Square> = (0..64).map(Square).collect();

    let wk = take(r, &mut free);
    let bk = take(r, &mut free);
    pos.board.set(wk, Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(bk, Some(Piece { color: Color::Black, kind: PieceKind::King }));

    for _ in 0..(1 + r.range(3)) {
        let ring: Vec<Square> =
            free.iter().copied().filter(|&s| adjacent(s, wk) || adjacent(s, bk)).collect();
        if ring.is_empty() {
            break;
        }
        let sq = ring[r.range(ring.len())];
        place(r, &mut free, &mut pos, sq);
    }
    for _ in 0..(1 + r.range(4)) {
        if free.is_empty() {
            break;
        }
        let sq = take(r, &mut free);
        place(r, &mut free, &mut pos, sq);
    }

    pos.side_to_move = if r.next_u8() & 1 == 0 { Color::White } else { Color::Black };
    pos.ply = 8;
    let our_king = if pos.side_to_move == Color::White { wk } else { bk };
    for _ in 0..r.range(3) {
        let square = if r.next_u8() & 1 == 0 {
            let near: Vec<Square> =
                (0..64).map(Square).filter(|&s| s == our_king || adjacent(s, our_king)).collect();
            near[r.range(near.len())]
        } else {
            Square((r.next_u8() as usize % 64) as u8)
        };
        let kind = if r.next_u8() & 1 == 0 { SpellKind::Freeze } else { SpellKind::Jump };
        if pos.fields.iter().any(|f| f.kind == kind && f.square == square) {
            continue;
        }
        if kind == SpellKind::Jump && pos.board.get(square).is_none() {
            continue;
        }
        pos.fields.push(SpellField {
            square,
            owner: pos.side_to_move.opposite(),
            kind,
            expires_after_ply: pos.ply,
        });
    }
    pos
}

/// Exactly what `check_kind_over_ordered` in `tests/spell_delta_soundness.rs`
/// checks: whenever the delta returns `Complete`, it must agree with the
/// rescan in set *and* emission order.
fn rescan_oracle(pos: &Position, cast: SpellCast, baseline: &[PieceMove]) -> Vec<PieceMove> {
    let mut next = *pos;
    next.fields.push(SpellField {
        square: cast.square,
        owner: pos.side_to_move,
        kind: cast.kind,
        expires_after_ply: pos.ply + 1,
    });
    legal_moves(&next)
        .into_iter()
        .filter(|mv| {
            let is_cap = pos.board.get(mv.to).is_some() || mv.is_en_passant;
            is_cap && !baseline.contains(mv)
        })
        .collect()
}

fn key(mv: &PieceMove) -> (u8, u8, u8, bool, bool) {
    (mv.from.0, mv.to.0, mv.promotion.map(|p| p as u8).unwrap_or(255), mv.is_en_passant, mv.is_castle)
}

fuzz_target!(|data: &[u8]| {
    let mut r = ByteReader::new(data);
    let pos = decode_position(&mut r);
    let baseline = legal_moves(&pos);
    for square in spells::freeze_targets(&pos, pos.side_to_move) {
        let cast = SpellCast { kind: SpellKind::Freeze, square };
        let mut fast = Vec::new();
        match captures_enabled_by(&pos, cast, &baseline, &mut fast) {
            Delta::Complete => {
                let want = rescan_oracle(&pos, cast, &baseline);
                let mut got_sorted: Vec<_> = fast.iter().map(key).collect();
                let mut want_sorted: Vec<_> = want.iter().map(key).collect();
                got_sorted.sort();
                want_sorted.sort();
                assert_eq!(got_sorted, want_sorted, "SET MISMATCH for {cast:?} on {:?}", pos.board);
                let got_raw: Vec<_> = fast.iter().map(key).collect();
                let want_raw: Vec<_> = want.iter().map(key).collect();
                assert_eq!(got_raw, want_raw, "ORDER MISMATCH for {cast:?} on {:?}", pos.board);
            }
            Delta::NeedsRescan => {
                assert!(fast.is_empty(), "a declining call must not touch `out`");
            }
        }
    }
});

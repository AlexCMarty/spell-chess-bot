//! Differential test: whenever `captures_enabled_by` claims `Complete`, its answer
//! must equal what the rescan it replaces would have produced. `NeedsRescan` falls
//! through to that same rescan at every call site, so only `Complete` needs checking.

use spellchess_core::*;

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

fn random_legal_walk(seed: u64, plies: u32) -> Vec<Position> {
    let mut state = seed;
    let mut pos = Position::starting();
    let mut out = vec![pos];
    for _ in 0..plies {
        let turns = generate_turns(&pos);
        if turns.is_empty() {
            break;
        }
        let pick = (splitmix64(&mut state) as usize) % turns.len();
        pos = apply_turn(&pos, &turns[pick]);
        if pos.board.king_square(Color::White).is_none() || pos.board.king_square(Color::Black).is_none() {
            break;
        }
        out.push(pos);
    }
    out
}

fn sparse_endgame() -> Position {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
    pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
    pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
    pos.board.set(Square::from_str("h8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::Rook }));
    pos
}

fn battery() -> Vec<Position> {
    let mut out = vec![Position::starting(), sparse_endgame()];
    for seed in [1u64, 2, 7, 13, 42, 99, 1234, 31337] {
        out.extend(random_legal_walk(seed, 14));
    }
    out
}

fn with_field(pos: &Position, cast: SpellCast) -> Position {
    let mut next = *pos;
    next.fields.push(SpellField {
        square: cast.square,
        owner: pos.side_to_move,
        kind: cast.kind,
        expires_after_ply: pos.ply + 1,
    });
    next
}

/// Exactly what `generate_quiescence_from`'s rescan branch computes.
fn rescan_oracle(pos: &Position, cast: SpellCast, baseline: &[PieceMove]) -> Vec<PieceMove> {
    legal_moves(&with_field(pos, cast))
        .into_iter()
        .filter(|mv| {
            let is_cap = pos.board.get(mv.to).is_some() || mv.is_en_passant;
            is_cap && !baseline.contains(mv)
        })
        .collect()
}

fn sorted(moves: &[PieceMove]) -> Vec<(u8, u8, u8, bool, bool)> {
    let mut v: Vec<_> = moves
        .iter()
        .map(|mv| (mv.from.0, mv.to.0, mv.promotion.map(|p| p as u8).unwrap_or(255), mv.is_en_passant, mv.is_castle))
        .collect();
    v.sort();
    v
}

fn check_kind(kind: SpellKind) -> (u32, u32) {
    let (mut complete, mut declined) = (0u32, 0u32);
    for pos in battery() {
        let us = pos.side_to_move;
        if !match kind {
            SpellKind::Freeze => pos.spells(us).freeze.castable(),
            SpellKind::Jump => pos.spells(us).jump.castable(),
        } {
            continue;
        }
        let baseline = legal_moves(&pos);
        let targets = match kind {
            SpellKind::Freeze => spells::freeze_targets(&pos, us),
            SpellKind::Jump => spells::jump_targets(&pos, us),
        };
        for square in targets {
            let cast = SpellCast { kind, square };
            let mut fast = Vec::new();
            match captures_enabled_by(&pos, cast, &baseline, &mut fast) {
                Delta::NeedsRescan => {
                    declined += 1;
                    assert!(fast.is_empty(), "a declining call must not touch `out`");
                }
                Delta::Complete => {
                    complete += 1;
                    assert_eq!(
                        sorted(&fast),
                        sorted(&rescan_oracle(&pos, cast, &baseline)),
                        "delta disagreed with rescan for {cast:?}",
                    );
                }
            }
        }
    }
    (complete, declined)
}

/// The walk-from-start battery above almost never produces the open lines, live
/// spell fields and adjacent-to-king tangles where the delta's hard cases live: it
/// missed four distinct freeze unsoundnesses that hand-built fixtures in
/// `spell_delta.rs` caught. This battery samples sparse random positions instead --
/// two kings plus a handful of pieces on an otherwise empty board, with up to two
/// pre-existing spell fields -- which hits pins, x-rays and jump transparency far
/// more often per position.
fn sparse_random_positions(seed: u64, count: usize) -> Vec<Position> {
    const KINDS: [PieceKind; 5] =
        [PieceKind::Pawn, PieceKind::Knight, PieceKind::Bishop, PieceKind::Rook, PieceKind::Queen];
    let mut state = seed;
    let mut out = Vec::new();
    while out.len() < count {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.castle_rights =
            CastleRights { white_kingside: false, white_queenside: false, black_kingside: false, black_queenside: false };
        let mut free: Vec<Square> = (0..64).map(Square).collect();
        let take = |state: &mut u64, free: &mut Vec<Square>| free.remove((splitmix64(state) as usize) % free.len());

        let wk = take(&mut state, &mut free);
        let bk = take(&mut state, &mut free);
        pos.board.set(wk, Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(bk, Some(Piece { color: Color::Black, kind: PieceKind::King }));
        let extras = 2 + (splitmix64(&mut state) as usize) % 7;
        for _ in 0..extras {
            let sq = take(&mut state, &mut free);
            let kind = KINDS[(splitmix64(&mut state) as usize) % KINDS.len()];
            let color = if splitmix64(&mut state) & 1 == 0 { Color::White } else { Color::Black };
            // Pawns on the back ranks are unreachable in a real game.
            if kind == PieceKind::Pawn && (sq.rank() == 0 || sq.rank() == 7) {
                continue;
            }
            pos.board.set(sq, Some(Piece { color, kind }));
        }
        pos.side_to_move = if splitmix64(&mut state) & 1 == 0 { Color::White } else { Color::Black };
        pos.ply = 8;
        for _ in 0..(splitmix64(&mut state) % 3) {
            let square = Square((splitmix64(&mut state) % 64) as u8);
            let kind = if splitmix64(&mut state) & 1 == 0 { SpellKind::Freeze } else { SpellKind::Jump };
            // A live field of the same kind on the same square is not a legal state.
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
        out.push(pos);
    }
    out
}

fn check_kind_over(kind: SpellKind, positions: &[Position]) -> (u32, u32) {
    let (mut complete, mut declined) = (0u32, 0u32);
    for pos in positions {
        let us = pos.side_to_move;
        let baseline = legal_moves(pos);
        let targets = match kind {
            SpellKind::Freeze => spells::freeze_targets(pos, us),
            SpellKind::Jump => spells::jump_targets(pos, us),
        };
        for square in targets {
            let cast = SpellCast { kind, square };
            let mut fast = Vec::new();
            match captures_enabled_by(pos, cast, &baseline, &mut fast) {
                Delta::NeedsRescan => {
                    declined += 1;
                    assert!(fast.is_empty(), "a declining call must not touch `out`");
                }
                Delta::Complete => {
                    complete += 1;
                    assert_eq!(
                        sorted(&fast),
                        sorted(&rescan_oracle(pos, cast, &baseline)),
                        "delta disagreed with rescan for {cast:?} on {:?}",
                        pos.board,
                    );
                }
            }
        }
    }
    (complete, declined)
}

fn adjacent(a: Square, b: Square) -> bool {
    a != b && a.file().abs_diff(b.file()) <= 1 && a.rank().abs_diff(b.rank()) <= 1
}

/// The removed-defender mechanism lives in a geometry neither battery above
/// produces on purpose: an enemy piece standing *next to a king*, a defender of it
/// somewhere out on a ray, and -- the trap -- a king that is itself frozen, either
/// by the zone being cast or by an older field the opponent laid. This generator
/// forces all three: pieces are placed on the ring around a king by construction,
/// and spell fields are anchored on or beside a king half the time.
fn king_tangle_positions(seed: u64, count: usize) -> Vec<Position> {
    const KINDS: [PieceKind; 5] =
        [PieceKind::Pawn, PieceKind::Knight, PieceKind::Bishop, PieceKind::Rook, PieceKind::Queen];
    let mut state = seed;
    let mut out = Vec::new();
    while out.len() < count {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.castle_rights =
            CastleRights { white_kingside: false, white_queenside: false, black_kingside: false, black_queenside: false };
        let mut free: Vec<Square> = (0..64).map(Square).collect();
        let take = |state: &mut u64, free: &mut Vec<Square>| free.remove((splitmix64(state) as usize) % free.len());

        let wk = take(&mut state, &mut free);
        let bk = take(&mut state, &mut free);
        pos.board.set(wk, Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(bk, Some(Piece { color: Color::Black, kind: PieceKind::King }));

        let place = |state: &mut u64, free: &mut Vec<Square>, pos: &mut Position, sq: Square| {
            free.retain(|&s| s != sq);
            let kind = KINDS[(splitmix64(state) as usize) % KINDS.len()];
            let color = if splitmix64(state) & 1 == 0 { Color::White } else { Color::Black };
            if kind == PieceKind::Pawn && (sq.rank() == 0 || sq.rank() == 7) {
                return;
            }
            pos.board.set(sq, Some(Piece { color, kind }));
        };

        // 1-3 pieces crowding a king, so `KING_ATTACKS & enemy_bb` is rarely empty.
        for _ in 0..(1 + splitmix64(&mut state) % 3) {
            let ring: Vec<Square> =
                free.iter().copied().filter(|&s| adjacent(s, wk) || adjacent(s, bk)).collect();
            if ring.is_empty() {
                break;
            }
            let sq = ring[(splitmix64(&mut state) as usize) % ring.len()];
            place(&mut state, &mut free, &mut pos, sq);
        }
        // 1-4 pieces anywhere: the defenders and x-rays the freeze has to silence.
        for _ in 0..(1 + splitmix64(&mut state) % 4) {
            let sq = take(&mut state, &mut free);
            place(&mut state, &mut free, &mut pos, sq);
        }

        pos.side_to_move = if splitmix64(&mut state) & 1 == 0 { Color::White } else { Color::Black };
        pos.ply = 8;
        let our_king = if pos.side_to_move == Color::White { wk } else { bk };
        for _ in 0..(splitmix64(&mut state) % 3) {
            // Half the time anchor the field on or beside our own king, so the
            // "a king frozen by an older field has no moves at all" trap is live.
            let square = if splitmix64(&mut state) & 1 == 0 {
                let near: Vec<Square> =
                    (0..64).map(Square).filter(|&s| s == our_king || adjacent(s, our_king)).collect();
                near[(splitmix64(&mut state) as usize) % near.len()]
            } else {
                Square((splitmix64(&mut state) % 64) as u8)
            };
            let kind = if splitmix64(&mut state) & 1 == 0 { SpellKind::Freeze } else { SpellKind::Jump };
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
        out.push(pos);
    }
    out
}

#[test]
fn freeze_delta_matches_the_rescan_oracle_on_king_tangle_positions() {
    let positions = king_tangle_positions(0xBADCAFE, 400);
    let (complete, declined) = check_kind_over(SpellKind::Freeze, &positions);
    println!("freeze (king tangle): {complete} complete, {declined} declined");
    assert!(complete > 0, "freeze delta never returned Complete on the king-tangle battery");
}

#[test]
fn jump_delta_matches_the_rescan_oracle_on_king_tangle_positions() {
    let positions = king_tangle_positions(0x1CEB00DA, 400);
    let (complete, declined) = check_kind_over(SpellKind::Jump, &positions);
    println!("jump (king tangle): {complete} complete, {declined} declined");
    assert!(complete > 0, "jump delta never returned Complete on the king-tangle battery");
}

#[test]
fn freeze_delta_matches_the_rescan_oracle_on_sparse_random_positions() {
    let positions = sparse_random_positions(0xC0FFEE, 400);
    let (complete, declined) = check_kind_over(SpellKind::Freeze, &positions);
    println!("freeze (sparse): {complete} complete, {declined} declined");
    assert!(complete > 0, "freeze delta never returned Complete on the sparse battery");
}

#[test]
fn jump_delta_matches_the_rescan_oracle_on_sparse_random_positions() {
    let positions = sparse_random_positions(0x5EED, 400);
    let (complete, declined) = check_kind_over(SpellKind::Jump, &positions);
    println!("jump (sparse): {complete} complete, {declined} declined");
    assert!(complete > 0, "jump delta never returned Complete on the sparse battery");
}

#[test]
fn jump_delta_matches_the_rescan_oracle() {
    let (complete, declined) = check_kind(SpellKind::Jump);
    println!("jump: {complete} complete, {declined} declined");
    assert!(complete > 0, "jump delta never returned Complete -- it is not wired up");
}

#[test]
fn freeze_delta_matches_the_rescan_oracle() {
    let (complete, declined) = check_kind(SpellKind::Freeze);
    println!("freeze: {complete} complete, {declined} declined");
}

// ---------------------------------------------------------------------------
// Emission ORDER.
//
// Everything above compares sorted multisets, which is not the whole contract.
// `generate_quiescence_from` pushes the delta's moves into the turn list in
// emission order, and neither `dedup_captures` nor the quiescence loop sorts
// before consuming them -- so two answers with the same set but a different
// sequence make the search try captures in a different order, changing
// beta-cutoff order and qnode counts. The delta has to reproduce the rescan's
// *sequence*.
//
// `pseudo_legal_moves` walks `own.minus(frozen)` in ascending square order and
// emits each piece's destinations ascending (bitboard iteration), and
// `legal_moves` only filters, so the rescan is strictly ascending by (from, to).
// ---------------------------------------------------------------------------

fn raw(moves: &[PieceMove]) -> Vec<(u8, u8, u8, bool, bool)> {
    moves
        .iter()
        .map(|mv| (mv.from.0, mv.to.0, mv.promotion.map(|p| p as u8).unwrap_or(255), mv.is_en_passant, mv.is_castle))
        .collect()
}

/// Like `check_kind_over`, but checks the set *and then* the order, so a set
/// disagreement and an order disagreement fail with different messages.
fn check_kind_over_ordered(kind: SpellKind, positions: &[Position]) -> (u32, u32) {
    let (mut complete, mut declined) = (0u32, 0u32);
    for pos in positions {
        let us = pos.side_to_move;
        let baseline = legal_moves(pos);
        let targets = match kind {
            SpellKind::Freeze => spells::freeze_targets(pos, us),
            SpellKind::Jump => spells::jump_targets(pos, us),
        };
        for square in targets {
            let cast = SpellCast { kind, square };
            let mut fast = Vec::new();
            match captures_enabled_by(pos, cast, &baseline, &mut fast) {
                Delta::NeedsRescan => {
                    declined += 1;
                    assert!(fast.is_empty(), "a declining call must not touch `out`");
                }
                Delta::Complete => {
                    complete += 1;
                    let want = rescan_oracle(pos, cast, &baseline);
                    assert_eq!(
                        sorted(&fast),
                        sorted(&want),
                        "SET MISMATCH: delta disagreed with rescan for {cast:?} on {:?} fields {:?} stm {:?} ep {:?}",
                        pos.board,
                        pos.fields,
                        pos.side_to_move,
                        pos.en_passant,
                    );
                    assert_eq!(
                        raw(&fast),
                        raw(&want),
                        "ORDER MISMATCH (the sets agree): delta emitted the same captures in a \
                         different sequence than the rescan for {cast:?} on {:?} fields {:?} stm {:?} ep {:?}",
                        pos.board,
                        pos.fields,
                        pos.side_to_move,
                        pos.en_passant,
                    );
                }
            }
        }
    }
    (complete, declined)
}

/// The order contract, pinned deterministically rather than left to a random
/// battery that hits this geometry about once in ten thousand positions.
///
/// White Ra3 is pinned to Ka1 by Ra6; Black Nb2 stands beside Ka1 defended only
/// by Rb5. freeze@b6 covers a6 AND b5, so one cast fires both delta mechanisms:
/// the released rook takes Bd3 (mechanism 2, from-square a3 = 16) and the king
/// takes the now-undefended knight (mechanism 3, from-square a1 = 0). The
/// mechanisms run in that order, but the rescan emits ascending by from-square,
/// so the king's capture must come FIRST. Same set either way -- only the
/// sequence tells the two apart, which is why the sorted comparisons above
/// cannot see this.
#[test]
fn the_freeze_delta_emits_a_king_capture_before_a_higher_indexed_released_piece() {
    let mut pos = Position { board: Board::empty(), ..Position::starting() };
    pos.castle_rights =
        CastleRights { white_kingside: false, white_queenside: false, black_kingside: false, black_queenside: false };
    let sq = |s: &str| Square::from_str(s).unwrap();
    let mut put = |s: &str, color: Color, kind: PieceKind| pos.board.set(sq(s), Some(Piece { color, kind }));
    put("a1", Color::White, PieceKind::King);
    put("a3", Color::White, PieceKind::Rook);
    put("b2", Color::Black, PieceKind::Knight);
    put("b5", Color::Black, PieceKind::Rook);
    put("a6", Color::Black, PieceKind::Rook);
    put("d3", Color::Black, PieceKind::Bishop);
    put("h8", Color::Black, PieceKind::King);

    let baseline = legal_moves(&pos);
    let kxb2 = PieceMove::quiet(sq("a1"), sq("b2"));
    let rxd3 = PieceMove::quiet(sq("a3"), sq("d3"));
    assert!(!baseline.contains(&kxb2), "fixture is wrong: Rb5 must defend b2");
    assert!(!baseline.contains(&rxd3), "fixture is wrong: Ra3 must be pinned");

    let cast = SpellCast { kind: SpellKind::Freeze, square: sq("b6") };
    let want = rescan_oracle(&pos, cast, &baseline);
    assert_eq!(raw(&want), raw(&[kxb2, rxd3]), "fixture is wrong: the rescan must emit Kxb2 then Rxd3");

    let mut fast = Vec::new();
    assert_eq!(captures_enabled_by(&pos, cast, &baseline, &mut fast), Delta::Complete);
    // The set check the batteries do -- passes even with the ordering bug present.
    assert_eq!(sorted(&fast), sorted(&want), "SET MISMATCH");
    // The check that actually catches it.
    assert_eq!(raw(&fast), raw(&want), "ORDER MISMATCH (the sets agree)");
}

const DIRS: [(i8, i8); 8] = [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)];
const DENSE_KINDS: [PieceKind; 5] =
    [PieceKind::Pawn, PieceKind::Knight, PieceKind::Bishop, PieceKind::Rook, PieceKind::Queen];

/// Adds a live en-passant square when the geometry supports one. Neither battery
/// above ever produces one, so the `pos.en_passant.is_some()` decline in
/// `freeze_captures` and the en-passant arm of `legal_moves` went unexercised by
/// the random sweeps.
fn maybe_en_passant(pos: &mut Position, state: &mut u64) {
    if splitmix64(state) % 4 != 0 {
        return;
    }
    let (ep_rank, cap_rank) = if pos.side_to_move == Color::White { (5u8, 4u8) } else { (2u8, 3u8) };
    let file = (splitmix64(state) % 8) as u8;
    let ep = Square::new(file, ep_rank);
    let cap = Square::new(file, cap_rank);
    let victim = pos.board.get(cap);
    if pos.board.get(ep).is_none()
        && victim.is_some_and(|p| p.kind == PieceKind::Pawn && p.color != pos.side_to_move)
    {
        pos.en_passant = Some(ep);
    }
}

/// Ray-dense: most pieces sit on rays radiating from the side-to-move's king, and
/// jump fields are anchored on occupied squares, so pins, x-rays, ray growth and
/// jump transparency are common rather than rare.
fn ray_dense_positions(seed: u64, count: usize) -> Vec<Position> {
    let mut state = seed;
    let mut out = Vec::new();
    while out.len() < count {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.castle_rights =
            CastleRights { white_kingside: false, white_queenside: false, black_kingside: false, black_queenside: false };
        let wk = Square((splitmix64(&mut state) % 64) as u8);
        let bk = Square((splitmix64(&mut state) % 64) as u8);
        if wk == bk {
            continue;
        }
        pos.board.set(wk, Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(bk, Some(Piece { color: Color::Black, kind: PieceKind::King }));
        pos.side_to_move = if splitmix64(&mut state) & 1 == 0 { Color::White } else { Color::Black };
        let our_king = if pos.side_to_move == Color::White { wk } else { bk };

        let mut occupied: Vec<Square> = vec![wk, bk];
        // 3-9 pieces stacked along rays from our king.
        for _ in 0..(3 + splitmix64(&mut state) % 7) {
            let dir = DIRS[(splitmix64(&mut state) as usize) % 8];
            let dist = 1 + (splitmix64(&mut state) % 7) as i8;
            let f = our_king.file() as i8 + dir.0 * dist;
            let r = our_king.rank() as i8 + dir.1 * dist;
            if !(0..8).contains(&f) || !(0..8).contains(&r) {
                continue;
            }
            let sq = Square::new(f as u8, r as u8);
            if pos.board.get(sq).is_some() {
                continue;
            }
            let kind = DENSE_KINDS[(splitmix64(&mut state) as usize) % 5];
            let color = if splitmix64(&mut state) & 1 == 0 { Color::White } else { Color::Black };
            if kind == PieceKind::Pawn && (sq.rank() == 0 || sq.rank() == 7) {
                continue;
            }
            pos.board.set(sq, Some(Piece { color, kind }));
            occupied.push(sq);
        }
        // 0-4 pieces anywhere.
        for _ in 0..(splitmix64(&mut state) % 5) {
            let sq = Square((splitmix64(&mut state) % 64) as u8);
            if pos.board.get(sq).is_some() {
                continue;
            }
            let kind = DENSE_KINDS[(splitmix64(&mut state) as usize) % 5];
            let color = if splitmix64(&mut state) & 1 == 0 { Color::White } else { Color::Black };
            if kind == PieceKind::Pawn && (sq.rank() == 0 || sq.rank() == 7) {
                continue;
            }
            pos.board.set(sq, Some(Piece { color, kind }));
            occupied.push(sq);
        }
        pos.ply = 8;
        // 0-3 live fields; jumps land on occupied ray squares, so they are transparent
        // to a slider that actually points at something.
        for _ in 0..(splitmix64(&mut state) % 4) {
            let jumpish = splitmix64(&mut state) % 3 != 0;
            let kind = if jumpish { SpellKind::Jump } else { SpellKind::Freeze };
            let square = if jumpish {
                occupied[(splitmix64(&mut state) as usize) % occupied.len()]
            } else {
                Square((splitmix64(&mut state) % 64) as u8)
            };
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
        maybe_en_passant(&mut pos, &mut state);
        out.push(pos);
    }
    out
}

/// Cramped: everything inside a 4x4..6x6 window, so every piece shares files,
/// ranks and diagonals with several others. This is where "a slider gains a target
/// off its own pin ray" and multi-pin geometries actually occur.
fn cramped_positions(seed: u64, count: usize) -> Vec<Position> {
    let mut state = seed;
    let mut out = Vec::new();
    while out.len() < count {
        let mut pos = Position { board: Board::empty(), ..Position::starting() };
        pos.castle_rights =
            CastleRights { white_kingside: false, white_queenside: false, black_kingside: false, black_queenside: false };
        let w = 4 + (splitmix64(&mut state) % 3) as u8;
        let f0 = (splitmix64(&mut state) % (9 - w as u64)) as u8;
        let r0 = (splitmix64(&mut state) % (9 - w as u64)) as u8;
        let mut cells: Vec<Square> = Vec::new();
        for df in 0..w {
            for dr in 0..w {
                cells.push(Square::new(f0 + df, r0 + dr));
            }
        }
        let take = |state: &mut u64, free: &mut Vec<Square>| free.remove((splitmix64(state) as usize) % free.len());
        let wk = take(&mut state, &mut cells);
        let bk = take(&mut state, &mut cells);
        pos.board.set(wk, Some(Piece { color: Color::White, kind: PieceKind::King }));
        pos.board.set(bk, Some(Piece { color: Color::Black, kind: PieceKind::King }));
        let mut occupied = vec![wk, bk];
        for _ in 0..(4 + (splitmix64(&mut state) as usize) % 7) {
            if cells.is_empty() {
                break;
            }
            let sq = take(&mut state, &mut cells);
            let kind = DENSE_KINDS[(splitmix64(&mut state) as usize) % 5];
            let color = if splitmix64(&mut state) & 1 == 0 { Color::White } else { Color::Black };
            if kind == PieceKind::Pawn && (sq.rank() == 0 || sq.rank() == 7) {
                continue;
            }
            pos.board.set(sq, Some(Piece { color, kind }));
            occupied.push(sq);
        }
        pos.side_to_move = if splitmix64(&mut state) & 1 == 0 { Color::White } else { Color::Black };
        pos.ply = 8;
        // Every field lands on an occupied square here -- in a cramped board that is
        // also the only way a jump field is ever legal.
        for _ in 0..(splitmix64(&mut state) % 4) {
            let kind = if splitmix64(&mut state) % 3 != 0 { SpellKind::Jump } else { SpellKind::Freeze };
            let square = occupied[(splitmix64(&mut state) as usize) % occupied.len()];
            if pos.fields.iter().any(|f| f.kind == kind && f.square == square) {
                continue;
            }
            pos.fields.push(SpellField {
                square,
                owner: pos.side_to_move.opposite(),
                kind,
                expires_after_ply: pos.ply,
            });
        }
        maybe_en_passant(&mut pos, &mut state);
        out.push(pos);
    }
    out
}

#[test]
fn freeze_delta_matches_the_rescan_oracle_on_cramped_positions() {
    let positions = cramped_positions(0xD1B54A32D192ED03, 250);
    let (complete, declined) = check_kind_over_ordered(SpellKind::Freeze, &positions);
    println!("freeze (cramped): {complete} complete, {declined} declined");
    assert!(complete > 0, "freeze delta never returned Complete on the cramped battery");
}

#[test]
fn jump_delta_matches_the_rescan_oracle_on_cramped_positions() {
    let positions = cramped_positions(0x2545F4914F6CDD1D, 250);
    let (complete, declined) = check_kind_over_ordered(SpellKind::Jump, &positions);
    println!("jump (cramped): {complete} complete, {declined} declined");
    assert!(complete > 0, "jump delta never returned Complete on the cramped battery");
}

#[test]
fn freeze_delta_matches_the_rescan_oracle_on_ray_dense_positions() {
    let positions = ray_dense_positions(0x9E3779B97F4A7C15, 250);
    let (complete, declined) = check_kind_over_ordered(SpellKind::Freeze, &positions);
    println!("freeze (ray dense): {complete} complete, {declined} declined");
    assert!(complete > 0, "freeze delta never returned Complete on the ray-dense battery");
}

#[test]
fn jump_delta_matches_the_rescan_oracle_on_ray_dense_positions() {
    let positions = ray_dense_positions(0xBF58476D1CE4E5B9, 250);
    let (complete, declined) = check_kind_over_ordered(SpellKind::Jump, &positions);
    println!("jump (ray dense): {complete} complete, {declined} declined");
    assert!(complete > 0, "jump delta never returned Complete on the ray-dense battery");
}

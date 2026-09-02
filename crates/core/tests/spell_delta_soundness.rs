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

use spellchess_core::{Color, PieceKind, Position, SpellKind};

const fn splitmix64(mut x: u64) -> (u64, u64) {
    x = x.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB13315863);
    (x, z ^ (z >> 31))
}

struct Tables {
    piece: [[[u64; 64]; 6]; 2],
    side: u64,
    castle: [u64; 16],
    ep: [u64; 8],
    freeze_count: [[u64; 8]; 2],
    jump_count: [[u64; 4]; 2],
    freeze_lock: [[u64; 4]; 2],
    jump_lock: [[u64; 4]; 2],
    /// [kind][owner][square][remaining plies, 0..=7]
    field: [[[[u64; 8]; 64]; 2]; 2],
}

const fn fill_tables() -> Tables {
    let mut seed = 0xC0FF_EE15_C0FF_EE15;
    let mut piece = [[[0u64; 64]; 6]; 2];
    let mut c = 0;
    while c < 2 {
        let mut k = 0;
        while k < 6 {
            let mut sq = 0;
            while sq < 64 {
                let (s, z) = splitmix64(seed);
                seed = s;
                piece[c][k][sq] = z;
                sq += 1;
            }
            k += 1;
        }
        c += 1;
    }
    let (s, side) = splitmix64(seed);
    seed = s;
    let mut castle = [0u64; 16];
    let mut i = 0;
    while i < 16 {
        let (s, z) = splitmix64(seed);
        seed = s;
        castle[i] = z;
        i += 1;
    }
    let mut ep = [0u64; 8];
    i = 0;
    while i < 8 {
        let (s, z) = splitmix64(seed);
        seed = s;
        ep[i] = z;
        i += 1;
    }
    let mut freeze_count = [[0u64; 8]; 2];
    c = 0;
    while c < 2 {
        i = 0;
        while i < 8 {
            let (s, z) = splitmix64(seed);
            seed = s;
            freeze_count[c][i] = z;
            i += 1;
        }
        c += 1;
    }
    let mut jump_count = [[0u64; 4]; 2];
    c = 0;
    while c < 2 {
        i = 0;
        while i < 4 {
            let (s, z) = splitmix64(seed);
            seed = s;
            jump_count[c][i] = z;
            i += 1;
        }
        c += 1;
    }
    let mut freeze_lock = [[0u64; 4]; 2];
    c = 0;
    while c < 2 {
        i = 0;
        while i < 4 {
            let (s, z) = splitmix64(seed);
            seed = s;
            freeze_lock[c][i] = z;
            i += 1;
        }
        c += 1;
    }
    let mut jump_lock = [[0u64; 4]; 2];
    c = 0;
    while c < 2 {
        i = 0;
        while i < 4 {
            let (s, z) = splitmix64(seed);
            seed = s;
            jump_lock[c][i] = z;
            i += 1;
        }
        c += 1;
    }
    let mut field = [[[[0u64; 8]; 64]; 2]; 2];
    let mut kind = 0;
    while kind < 2 {
        c = 0;
        while c < 2 {
            let mut sq = 0;
            while sq < 64 {
                i = 0;
                while i < 8 {
                    let (s, z) = splitmix64(seed);
                    seed = s;
                    field[kind][c][sq][i] = z;
                    i += 1;
                }
                sq += 1;
            }
            c += 1;
        }
        kind += 1;
    }
    let _ = seed;
    Tables { piece, side, castle, ep, freeze_count, jump_count, freeze_lock, jump_lock, field }
}

const Z: Tables = fill_tables();

fn castle_index(pos: &Position) -> usize {
    let mut i = 0usize;
    if pos.castle_rights.white_kingside { i |= 1; }
    if pos.castle_rights.white_queenside { i |= 2; }
    if pos.castle_rights.black_kingside { i |= 4; }
    if pos.castle_rights.black_queenside { i |= 8; }
    i
}

/// Zobrist key for TT. Absolute ply / halfmove are omitted; live fields contribute
/// their remaining duration (`expires_after_ply - ply`) so two positions that
/// differ only by an expired clock still collide when the game state matches.
pub fn hash_position(pos: &Position) -> u64 {
    let mut h = 0u64;
    for color in [Color::White, Color::Black] {
        let ci = color.index();
        for kind in [
            PieceKind::Pawn, PieceKind::Knight, PieceKind::Bishop,
            PieceKind::Rook, PieceKind::Queen, PieceKind::King,
        ] {
            let ki = kind.index();
            let bb = pos.board.color_bb(color).intersect(pos.board.kind_bb(kind));
            for sq in bb.iter() {
                h ^= Z.piece[ci][ki][sq.0 as usize];
            }
        }
    }
    if pos.side_to_move == Color::Black {
        h ^= Z.side;
    }
    h ^= Z.castle[castle_index(pos)];
    if let Some(ep) = pos.en_passant {
        h ^= Z.ep[ep.file() as usize];
    }
    for color in [Color::White, Color::Black] {
        let ci = color.index();
        let spells = pos.spells(color);
        h ^= Z.freeze_count[ci][(spells.freeze.count as usize).min(7)];
        h ^= Z.jump_count[ci][(spells.jump.count as usize).min(3)];
        h ^= Z.freeze_lock[ci][(spells.freeze.lock as usize).min(3)];
        h ^= Z.jump_lock[ci][(spells.jump.lock as usize).min(3)];
    }
    for field in pos.fields.iter() {
        let kind = match field.kind {
            SpellKind::Freeze => 0,
            SpellKind::Jump => 1,
        };
        let remaining = field.expires_after_ply.saturating_sub(pos.ply).min(7) as usize;
        h ^= Z.field[kind][field.owner.index()][field.square.0 as usize][remaining];
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellchess_core::Position;

    #[test]
    fn identical_positions_hash_identically() {
        assert_eq!(hash_position(&Position::starting()), hash_position(&Position::starting()));
    }

    #[test]
    fn a_move_changes_the_hash() {
        let start = Position::starting();
        let turns = spellchess_core::generate_turns(&start);
        let after = spellchess_core::apply_turn(&start, &turns[0]);
        assert_ne!(hash_position(&start), hash_position(&after));
    }

    #[test]
    fn hash_ignores_absolute_ply_when_no_fields() {
        let mut shifted = Position::starting();
        shifted.ply = 12;
        shifted.halfmove_clock = 7;
        assert_eq!(hash_position(&Position::starting()), hash_position(&shifted));
    }
}

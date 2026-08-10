use crate::board::Board;
use crate::types::{Color, Square};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastleRights {
    pub white_kingside: bool,
    pub white_queenside: bool,
    pub black_kingside: bool,
    pub black_queenside: bool,
}

impl CastleRights {
    pub fn all() -> CastleRights {
        CastleRights { white_kingside: true, white_queenside: true, black_kingside: true, black_queenside: true }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpellKind {
    Freeze,
    Jump,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpellCounter {
    pub count: u8,
    pub lock: u8,
}

impl SpellCounter {
    pub fn castable(self) -> bool {
        self.count > 0 && self.lock == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpellState {
    pub freeze: SpellCounter,
    pub jump: SpellCounter,
}

impl SpellState {
    pub fn starting() -> SpellState {
        SpellState {
            freeze: SpellCounter { count: 5, lock: 0 },
            jump: SpellCounter { count: 2, lock: 0 },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpellField {
    pub square: Square,
    pub owner: Color,
    pub kind: SpellKind,
    /// Active for every ply up to and including this one (see rules/20-spell-system.md#field-lifetime).
    pub expires_after_ply: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    pub board: Board,
    pub side_to_move: Color,
    pub castle_rights: CastleRights,
    pub en_passant: Option<Square>,
    pub halfmove_clock: u32,
    pub ply: u64,
    pub white_spells: SpellState,
    pub black_spells: SpellState,
    pub fields: Vec<SpellField>,
}

impl Position {
    pub fn starting() -> Position {
        Position {
            board: Board::starting(),
            side_to_move: Color::White,
            castle_rights: CastleRights::all(),
            en_passant: None,
            halfmove_clock: 0,
            ply: 0,
            white_spells: SpellState::starting(),
            black_spells: SpellState::starting(),
            fields: Vec::new(),
        }
    }

    pub fn spells(&self, color: Color) -> SpellState {
        match color {
            Color::White => self.white_spells,
            Color::Black => self.black_spells,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starting_position_has_canonical_spell_counts() {
        let pos = Position::starting();
        assert_eq!(pos.spells(Color::White).freeze, SpellCounter { count: 5, lock: 0 });
        assert_eq!(pos.spells(Color::White).jump, SpellCounter { count: 2, lock: 0 });
        assert_eq!(pos.spells(Color::Black), pos.spells(Color::White));
    }

    #[test]
    fn starting_position_white_to_move_no_fields() {
        let pos = Position::starting();
        assert_eq!(pos.side_to_move, Color::White);
        assert!(pos.fields.is_empty());
    }

    #[test]
    fn spell_counter_castable_requires_count_and_no_lock() {
        assert!(SpellCounter { count: 1, lock: 0 }.castable());
        assert!(!SpellCounter { count: 0, lock: 0 }.castable());
        assert!(!SpellCounter { count: 1, lock: 2 }.castable());
    }
}

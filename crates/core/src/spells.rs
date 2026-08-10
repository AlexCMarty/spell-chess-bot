use crate::position::{Position, SpellField, SpellKind};
use crate::types::{Color, Square};

pub fn freeze_zone(target: Square) -> Vec<Square> {
    let mut out = Vec::new();
    let (tf, tr) = (target.file() as i8, target.rank() as i8);
    for df in -1..=1 {
        for dr in -1..=1 {
            let (f, r) = (tf + df, tr + dr);
            if (0..8).contains(&f) && (0..8).contains(&r) {
                out.push(Square::new(f as u8, r as u8));
            }
        }
    }
    out
}

fn field_active(pos: &Position, field: &SpellField) -> bool {
    pos.ply <= field.expires_after_ply
}

pub fn is_square_frozen(pos: &Position, square: Square) -> bool {
    pos.fields.iter().any(|f| f.kind == SpellKind::Freeze && field_active(pos, f) && freeze_zone(f.square).contains(&square))
}

pub fn freeze_targets(pos: &Position, color: Color) -> Vec<Square> {
    if !pos.spells(color).freeze.castable() {
        return Vec::new();
    }
    (0..64).map(Square).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Square;

    #[test]
    fn corner_freeze_clips_to_2x2() {
        let a1 = Square::from_str("a1").unwrap();
        let zone = freeze_zone(a1);
        assert!(zone.contains(&Square::from_str("a1").unwrap()));
        assert!(zone.contains(&Square::from_str("a2").unwrap()));
        assert!(zone.contains(&Square::from_str("b2").unwrap()));
        assert!(!zone.contains(&Square::from_str("c3").unwrap()));
        assert_eq!(zone.len(), 4);
    }

    #[test]
    fn center_freeze_covers_3x3() {
        assert_eq!(freeze_zone(Square::from_str("d5").unwrap()).len(), 9);
    }
}

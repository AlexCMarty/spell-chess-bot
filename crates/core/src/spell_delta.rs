//! Direct computation of the captures a spell cast newly makes legal, replacing
//! the "build a hypothetical position and run a full `legal_moves` rescan" approach
//! that dominated search time (36-41us per leaf node).
//!
//! See docs/superpowers/specs/2026-09-01-spell-capture-delta-design.md.

use crate::legal::SpellCast;
use crate::movegen::PieceMove;
use crate::position::Position;

/// Whether the fast path could settle the question for a given cast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delta {
    /// `out` gained exactly the newly-legal captures. Authoritative.
    Complete,
    /// The fast path declined. The caller must run the rescan; `out` is untouched.
    NeedsRescan,
}

/// Captures that `cast` newly makes legal for `pos.side_to_move`, excluding
/// anything already legal in `baseline`. Appends to `out`.
///
/// Returning `NeedsRescan` is always safe: correctness never depends on this
/// function being exhaustive, only speed does.
///
/// Invariant upheld for every `NeedsRescan` return, including from future
/// (Task 3-5) code paths that speculatively push candidates into `out` before
/// concluding they can't settle the question: `out` is left exactly as it was
/// on entry. This function records `out`'s length at entry and truncates back
/// to it on every declining exit, so later additions to this function inherit
/// the invariant instead of having to re-establish it by hand.
pub fn captures_enabled_by(
    pos: &Position,
    cast: SpellCast,
    baseline: &[PieceMove],
    out: &mut Vec<PieceMove>,
) -> Delta {
    let start_len = out.len();
    let _ = (pos, cast, baseline);
    out.truncate(start_len);
    Delta::NeedsRescan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legal::legal_moves;
    use crate::position::SpellKind;
    use crate::types::Square;

    #[test]
    fn declining_leaves_the_output_buffer_untouched() {
        let pos = Position::starting();
        let baseline = legal_moves(&pos);
        let cast = SpellCast { kind: SpellKind::Freeze, square: Square::from_str("d4").unwrap() };
        let mut out = vec![baseline[0]];
        let before = out.clone();
        if captures_enabled_by(&pos, cast, &baseline, &mut out) == Delta::NeedsRescan {
            assert_eq!(out, before, "a declining call must not touch `out`");
        }
    }
}

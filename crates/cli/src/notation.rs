//! Turn notation moved to `spellchess-core` so the WASM front end can format and
//! parse turns without depending on this crate. Re-exported here so CLI callers
//! keep working against `crate::notation`.
pub use spellchess_core::notation::{format_turn, parse_turn};

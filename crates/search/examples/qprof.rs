//! Where `generate_quiescence_from`'s ~15.5us per call actually goes, measured over a
//! real search rather than a synthetic loop so the guard/delta/rescan mix is the one
//! the engine really sees.
//!
//!   cargo run --release -p spellchess-search --example qprof \
//!       --features spellchess-core/qprofile
//!
//! Without the feature this prints a reminder and exits. Depth is the first argument
//! (default 5 -- deep enough for a representative mix, quick enough to iterate on).

use spellchess_core::Position;
use spellchess_search::search::{search, Budget};

fn main() {
    let depth: u32 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(5);
    let start = std::time::Instant::now();
    let best = search(&Position::starting(), Budget::Depth(depth));
    println!("depth {depth} in {:.3?} -> {:?}", start.elapsed(), best.map(|(_, s)| s));
    spellchess_core::qprof::dump();
}

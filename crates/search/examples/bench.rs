//! Wall-clock for a fixed-depth search from the starting position, with no CLI in the
//! link unit.
//!
//!   cargo run --release -p spellchess-search --example bench -- 6
//!
//! Threads come from `SPELLCHESS_THREADS`, defaulting to every core. Use
//! `SPELLCHESS_THREADS=1` for any measurement you want to compare against another
//! commit: Lazy-SMP timings vary run to run, single-threaded ones do not.
//!
//! Prefer this over driving `spellchess`'s `go --depth N` for perf work: one process,
//! one number, no REPL in the way.
//!
//! `cargo build --release --workspace --examples` builds ONLY example targets -- it
//! does not rebuild `target/release/spellchess`. Timing that stale binary against a
//! freshly built example is how 2026-09-02 spent an hour "discovering" a 20% codegen
//! difference between the two that did not exist. Use `--bins --examples`, or check
//! the mtime.
use spellchess_core::Position;
use spellchess_search::search::{default_threads, search_smp, Budget};

fn main() {
    let depth: u32 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(6);
    let start = std::time::Instant::now();
    let threads = default_threads();
    let best = search_smp(&Position::starting(), Budget::Depth(depth), threads);
    println!("depth {depth} on {threads} thread(s): {:.3?} -> {:?}", start.elapsed(), best.map(|(_, s)| s));
}

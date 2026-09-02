//! Opt-in attribution counters for `generate_quiescence_from` -- the search's single
//! hottest function (~15.5us per call, once per quiescence entry, millions of times
//! per search). `hotcost` gives its *total* per-call cost; this says where that total
//! goes, which is what tells you whether the next optimisation is worth writing.
//!
//! Off by default and free when off: without `--features qprofile` every macro below
//! expands to the bare expression and none of these statics exist.
//!
//! ```text
//! cargo run --release -p spellchess-search --example qprof \
//!     --features spellchess-core/qprofile
//! ```
//!
//! The loops are timed whole and their iterations merely counted, so the clock
//! overhead (~25ns per `Instant::now`) lands on the handful of per-node scopes rather
//! than on the ~100 per-target iterations inside them. Delta calls are timed
//! individually because "how often does the guard even let one through" is the
//! question this exists to answer.

#[cfg(feature = "qprofile")]
mod imp {
    use std::sync::atomic::{AtomicU64, Ordering};

    pub struct Bucket {
        pub name: &'static str,
        pub calls: AtomicU64,
        pub nanos: AtomicU64,
    }

    impl Bucket {
        pub const fn new(name: &'static str) -> Bucket {
            Bucket { name, calls: AtomicU64::new(0), nanos: AtomicU64::new(0) }
        }
        pub fn hit(&self, n: u64) {
            self.calls.fetch_add(n, Ordering::Relaxed);
        }
        pub fn record(&self, nanos: u64) {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.nanos.fetch_add(nanos, Ordering::Relaxed);
        }
    }

    macro_rules! buckets {
        ($($name:ident),* $(,)?) => {
            $(pub static $name: Bucket = Bucket::new(stringify!($name));)*
            pub static ALL: &[&Bucket] = &[$(&$name),*];
        };
    }

    buckets!(
        TOTAL,
        CTX_NEW,
        BASELINE_CAPTURES,
        JUMP_LOOP,
        JUMP_TARGET,
        JUMP_GUARD_PASS,
        JUMP_DELTA,
        JUMP_RESCAN,
        FREEZE_RECAP_LOOP,
        REL_FREEZE_TARGETS,
        FREEZE_LOOP,
        FREEZE_TARGET,
        FREEZE_GUARD_PASS,
        FREEZE_DELTA,
        FREEZE_RESCAN,
    );

    pub fn dump() {
        let total = TOTAL.nanos.load(Ordering::Relaxed).max(1) as f64;
        let calls = TOTAL.calls.load(Ordering::Relaxed).max(1);
        println!(
            "\n{:<20} {:>12} {:>14} {:>12} {:>8}",
            "bucket", "calls", "total ms", "ns/gen-call", "% total"
        );
        for b in ALL {
            let c = b.calls.load(Ordering::Relaxed);
            let n = b.nanos.load(Ordering::Relaxed) as f64;
            println!(
                "{:<20} {:>12} {:>14.1} {:>12.1} {:>7.1}%",
                b.name,
                c,
                n / 1e6,
                n / calls as f64,
                100.0 * n / total,
            );
        }
        println!(
            "\n(rows with 0.0 ms are counters, not timers. ns/gen-call divides by \
             TOTAL's {calls} calls, so it reads as \"cost per generate_quiescence_from\".)"
        );
    }
}

#[cfg(feature = "qprofile")]
pub use imp::*;

#[cfg(not(feature = "qprofile"))]
pub fn dump() {
    println!("qprof: built without --features spellchess-core/qprofile; nothing recorded.");
}

/// Time `$e`, attributing the elapsed nanoseconds to bucket `$b`. Expands to just
/// `$e` without the feature.
#[macro_export]
macro_rules! qtime {
    ($b:ident, $e:expr) => {{
        #[cfg(feature = "qprofile")]
        {
            let __start = std::time::Instant::now();
            let __v = $e;
            $crate::qprof::$b.record(__start.elapsed().as_nanos() as u64);
            __v
        }
        #[cfg(not(feature = "qprofile"))]
        {
            $e
        }
    }};
}

/// Bump bucket `$b`'s call count by 1 (or by `$n`). Expands to nothing without the
/// feature.
#[macro_export]
macro_rules! qcount {
    ($b:ident) => {
        $crate::qcount!($b, 1)
    };
    ($b:ident, $n:expr) => {{
        #[cfg(feature = "qprofile")]
        {
            $crate::qprof::$b.hit($n as u64);
        }
    }};
}

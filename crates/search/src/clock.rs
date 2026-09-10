//! `Instant` that works in a browser.
//!
//! `std::time::Instant::now()` panics on `wasm32-unknown-unknown`, and
//! `search_smp` calls it on its first line regardless of budget. `web_time`
//! re-exports `std::time` unchanged off wasm, so this switch costs native
//! builds nothing -- not even a dependency, since it is target-gated.
//!
//! `Duration` is re-exported alongside `Instant` and is `std::time::Duration`
//! on every target, so `Budget::Time` stays interchangeable with std durations
//! built by callers.

#[cfg(target_arch = "wasm32")]
pub use web_time::{Duration, Instant};

#[cfg(not(target_arch = "wasm32"))]
pub use std::time::{Duration, Instant};

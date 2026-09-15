//! The browser boundary. A shell over `view`: every method here parses its
//! arguments, calls one `view` function, and hands back a JSON string.
//!
//! Kept deliberately thin because nothing in this file can be reached by
//! `cargo test` -- the logic worth testing lives in `view.rs`, which can.

pub mod json;
pub mod view;

#[cfg(target_arch = "wasm32")]
mod bindings {
    use crate::view;
    use spellchess_core::{apply_turn, Position};
    use spellchess_search::clock::{Duration, Instant};
    use spellchess_search::search::{search_with_progress, Budget};
    use wasm_bindgen::prelude::*;

    /// Installed once at module load so a panic inside the engine surfaces as a
    /// readable console message instead of an opaque `unreachable executed`.
    #[wasm_bindgen(start)]
    pub fn start() {
        console_error_panic_hook::set_once();
    }

    /// Nanoseconds per `timed_out()` clock read.
    ///
    /// `Budget::Time` -- the mode the site uses -- leaves `deadline` as `Some`,
    /// so `Instant::now()` runs once per node *and* once per qnode. Natively
    /// that is a vDSO read; in the browser it is a JS boundary crossing through
    /// `performance.now()`. Step 5 sizes that against the search budget.
    ///
    /// Measurement only. It shares no state with the search and cannot move a
    /// node count.
    #[wasm_bindgen]
    pub fn bench_clock(iters: u32) -> f64 {
        let deadline = Instant::now() + Duration::from_secs(3600);
        let t0 = Instant::now();
        let mut hits = 0u32;
        for _ in 0..iters {
            // Deliberately the exact shape of `timed_out`'s second line.
            if Instant::now() >= deadline {
                hits += 1;
            }
        }
        let elapsed = t0.elapsed().as_nanos() as f64;
        // Keeps the loop observable so LLVM cannot delete it. The deadline is an
        // hour out, so this never returns -1.
        if hits > 0 {
            return -1.0;
        }
        elapsed / iters as f64
    }

    /// The authoritative game. One of these lives in the Web Worker; the main
    /// thread keeps no game state of its own.
    #[wasm_bindgen]
    pub struct Game {
        pos: Position,
        history: Vec<Position>,
    }

    #[wasm_bindgen]
    impl Game {
        #[wasm_bindgen(constructor)]
        pub fn new() -> Game {
            Game { pos: Position::starting(), history: Vec::new() }
        }

        pub fn reset(&mut self) {
            self.pos = Position::starting();
            self.history.clear();
        }

        /// Steps back one turn. Returns false when there is nothing to undo, so
        /// the UI can leave the button disabled rather than guess.
        pub fn undo(&mut self) -> bool {
            match self.history.pop() {
                Some(prev) => {
                    self.pos = prev;
                    true
                }
                None => false,
            }
        }

        pub fn state_json(&self) -> String {
            view::state_json(&self.pos)
        }

        pub fn legal_turns_json(&self) -> String {
            view::turns_json(&spellchess_core::generate_turns(&self.pos))
        }

        /// Applies a turn described by the boundary shapes. Returns the new state
        /// JSON. Errors become JavaScript exceptions: the UI only offers turns
        /// from `legal_turns_json`, so an error here means a front-end bug, and
        /// failing loudly beats applying something else.
        pub fn apply_turn(
            &mut self,
            from: &str,
            to: &str,
            promo: Option<String>,
            spell_kind: Option<String>,
            spell_at: Option<String>,
        ) -> Result<String, JsError> {
            let resolve = || -> Result<_, String> {
                let from = view::parse_square(from)?;
                let to = view::parse_square(to)?;
                let promo = view::parse_promo(promo.as_deref())?;
                let spell = view::parse_spell(spell_kind.as_deref(), spell_at.as_deref())?;
                view::find_turn(&self.pos, from, to, promo, spell)
            };
            let turn = resolve().map_err(|e| JsError::new(&e))?;
            self.history.push(self.pos.clone());
            self.pos = apply_turn(&self.pos, &turn);
            Ok(view::state_json(&self.pos))
        }

        /// Searches for `millis`, invoking `on_progress(depth, score, turn_json)`
        /// after each completed iteration. Returns the chosen turn as JSON, or
        /// `"null"` when the position has no legal turn.
        ///
        /// `threads` is hard-coded to 1: the browser cannot spawn them, and
        /// Lazy-SMP is a measured net loss on this engine anyway.
        pub fn search(&self, millis: u32, on_progress: &js_sys::Function) -> String {
            let budget = Budget::Time(Duration::from_millis(millis as u64));
            let mut report = |depth: u32, score: i32, turn: &spellchess_core::Turn| {
                // Deliberately swallowed: `on_progress` only repaints a cosmetic
                // analysis panel, and an error there (or the callback throwing)
                // must never unwind the search itself.
                let _ = on_progress.call3(
                    &JsValue::NULL,
                    &JsValue::from(depth),
                    &JsValue::from(score),
                    &JsValue::from_str(&view::turn_json(turn)),
                );
            };
            match search_with_progress(&self.pos, budget, 1, &mut report) {
                Some((turn, _)) => view::turn_json(&turn),
                None => "null".to_string(),
            }
        }
    }
}

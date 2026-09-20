---
paths:
  - "web/**"
  - "crates/wasm/**"
---

- **Asset paths in `web/` must be relative, never a leading `/`.** The site is served
  from a subpath (GitHub Pages project site); an absolute path breaks it with no local
  symptom, since `python3 -m http.server` at the repo root won't show the mismatch.
- **The worker's `undo` reply is a bare `true`/`false`, not `{data: ...}`.** Every other
  command replies `{id, ok, data}`. `app.js` tests the boolean directly to decide whether
  to keep unwinding history; "normalising" `undo` to match the others breaks that loop
  silently.
- **`board[]` uses `""` for an empty square, never `null`,** and is indexed
  `rank * 8 + file` (index 0 = a1, 63 = h8). `app.js` relies on both.
- **`cargo test --workspace` never compiles this crate's browser half.** The
  `#[cfg(target_arch = "wasm32")]` `bindings` module in `crates/wasm/src/lib.rs` only
  type-checks under `wasm-pack build crates/wasm --target web`. Run that (or trust CI's
  per-PR run of it) before believing a change to `lib.rs` compiles.
- **`std::time::Instant::now()` panics on wasm32.** Use `crates/search/src/clock.rs`'s
  wasm-aware clock instead of calling it directly from code that can run in the browser.
- **Don't remove the `cfg(not(target_arch = "wasm32"))` guard around `target-cpu=native`**
  in `.cargo/config.toml`. Applying it unscoped breaks wasm-bindgen's reference-types
  detection.

Canonical reference: [`docs/WEB.md`](../../docs/WEB.md).

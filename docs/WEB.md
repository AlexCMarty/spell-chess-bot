# The browser build

How `crates/wasm` and `web/` fit together. For the engine itself start at
[`ARCHITECTURE.md`](ARCHITECTURE.md); for the *rules* start at
[`rules/INDEX.md`](../rules/INDEX.md).

The site is the shipped product: the engine compiled to WebAssembly, published at
<https://alexcmarty.github.io/spell-chess-bot/>, running entirely client-side with no
server component.

## The shape

One WASM instance, inside a **Web Worker**, owns the game. The main thread is a pure
view: it sends commands and renders the JSON that comes back. It holds no authoritative
game state of its own.

```
main thread                      Web Worker
-----------                      ----------
app.js  ── command ─────────────▶ worker.js
        ◀──── state JSON ─────── spellchess_wasm::Game
                                   └── Position + history (authoritative)
```

Two decisions are load-bearing and both are easy to "simplify" wrongly:

- **The worker owns the state because there is no position serializer.** The `fen` REPL
  command is `format!("{:?}")` with no matching parser, so nothing can round-trip a
  `Position` through a string. Answering legality on the main thread would mean building
  that serializer *and* keeping two copies of the game state that can disagree. The cost
  of the chosen design is a sub-millisecond `postMessage` to ask "what are my legal
  moves", which is cheaper than a class of desync bugs.
- **The search cannot run on the main thread.** It takes seconds and would freeze the
  page. Making it yield periodically would mean threading yield points through
  `negamax`, which risks the node-count invariant.

**Legality comes from `generate_turns`** — the exhaustive slow oracle, not one of the
search generators. It runs once per user turn, where its cost is irrelevant, and being
the reference generator is what makes it impossible for the UI to offer an illegal turn.

## The worker command protocol

Undocumented on both sides until now; it exists only as executable code in
`web/worker.js` and `web/app.js`.

**Request** (main thread → worker): `{id, cmd, ...args}`, where `id` is a
monotonically increasing integer the caller uses to match up the reply.

**Reply** (worker → main thread): `{id, ok: true, data}` or `{id, ok: false, error}`.

**Out-of-band**, with no `id` and unsolicited: `{type: "progress", depth, score, turn}`,
emitted once per completed search depth. `app.js` routes any message carrying `type`
to the progress handler before it consults the pending-request map.

| `cmd` | Args | `data` |
|---|---|---|
| `state` | — | the state object below |
| `turns` | — | every legal turn, as `turn_json` objects |
| `apply` | `from`, `to`, `promo?`, `spell?` | the new state object |
| `search` | `millis` | best turn + score (also emits `progress` messages) |
| `undo` | — | **a bare `true`/`false`**, not an object — `false` when history is empty |
| `reset` | — | the new state object |

That `undo` asymmetry is real and depended upon (`app.js` tests the boolean directly to
decide whether to keep unwinding). Anything that "normalises" it to an object breaks the
undo loop silently.

Squares cross the boundary as algebraic strings (`"e4"`). `spell` is `null` for no cast
or `{kind: "freeze"|"jump", at: "e4"}`. `promo` is `null` or one of `"q"|"r"|"b"|"n"`.
These are the only shapes the boundary accepts; the JSON layer rejects anything else
with an error string rather than guessing.

## The state JSON

Built by `state_json` in `crates/wasm/src/view.rs`, consumed by `web/app.js`.

```js
{
  board: [ /* 64 entries, index 0 = a1, index 63 = h8 */ ],
  sideToMove: "white" | "black",
  ply: 0,
  spells: { white: {freeze: {count, lock}, jump: {count, lock}}, black: {…} },
  fields: [ {square, owner, kind, expiresAfterPly} ],
  status: { kind, winner? }
}
```

Things a consumer has to know and cannot infer:

- **`board` is indexed `rank * 8 + file`** — index 0 is a1, 63 is h8. An empty square is
  the empty string `""`, **not** `null`; `app.js` relies on that.
- **`lock` is a cooldown in the holder's own turns**, set to 3 on cast and decremented
  once per opponent turn, so the number rendered on your own turn is "this many more of
  *your* turns to wait". A spell is castable when `count > 0 && lock == 0`.
- **`count` never replenishes.** Only `lock` resets. See
  [`rules/20-spell-system.md`](../rules/20-spell-system.md#spell-economy).
- **`status.kind` is one of `inProgress | stalemate | checkmate | kingCaptured`**, with
  `winner` present only on the latter two. `app.js` switches on these four strings; a
  fifth variant added in `core` falls through and silently renders a finished game as
  still in progress.
- **`expiresAfterPly` is emitted and currently read by nobody.** Either a consumer is
  missing or the field is dead; nothing records which.
- `turn_json`'s `text` comes from `format_turn` (bare coordinates). `app.js` deliberately
  re-derives its own notation for the move list rather than using it — a live divergence
  between the two sides.

## Testability rule

Every JSON-building function is a plain Rust `fn` over ordinary types, with no
`wasm_bindgen` types in its signature. The `#[wasm_bindgen]` methods in `lib.rs` are
thin shells over them.

That is why `view.rs` carries the tests and `lib.rs` carries almost no logic: the whole
JSON layer stays under `cargo test -p spellchess-wasm` on the **native** target, with no
browser. Keep new logic on the `view.rs` side of that line.

The corollary is the trap in the next section: nothing native compiles the other half.

## Traps

- **`cargo test --workspace` does not compile the browser build.** The entire `bindings`
  module in `crates/wasm/src/lib.rs` is behind `#[cfg(target_arch = "wasm32")]`. Only
  `wasm-pack build` type-checks it; CI does that on every PR
  (`.github/workflows/pages.yml`). This mirrors the `crates/core/fuzz` carve-out.
- **`target-cpu=native` must not reach wasm32.** `.cargo/config.toml` scopes it to
  `cfg(not(target_arch = "wasm32"))` deliberately — under `[build]` it perturbs
  reference-types feature detection and wasm-bindgen fails with "failed to find the
  `__wbindgen_externref_table_dealloc` function".
- **`std::time::Instant::now()` panics on wasm32.** `search_smp` called it
  unconditionally, before branching on `Budget`, so a clock fix was required even for a
  pure depth-limited search. `crates/search/src/clock.rs` swaps in `web-time` for wasm.
- **All asset paths in `web/` must be relative.** The site is served from a
  subpath; a leading `/` breaks it, with no local symptom.
- **A JS throw inside the `search` progress callback is swallowed** by design
  (`lib.rs`) so it cannot unwind across the wasm boundary mid-search. The visible effect
  is that the analysis panel stops updating while the search runs to completion, with no
  console breadcrumb. `worker.js` does `JSON.parse` and `postMessage` inside that
  callback, so those are the throws being eaten.
- **Multithreading is unavailable in the browser** — and nothing is lost, because
  `default_threads()` already returns 1 and Lazy-SMP is a measured net loss on this
  engine. `std::thread::scope` with zero spawns is fine on wasm, so no cfg bypass is
  needed.

## Build and deploy

```sh
# one-time
rustup target add wasm32-unknown-unknown
# and install wasm-pack (CI pins v0.15.0 from the prebuilt tarball; `cargo install
# wasm-pack` also works but compiles from source)

wasm-pack build crates/wasm --target web --out-dir ../../web/pkg --release
python3 -m http.server -d web 8099
```

Then <http://localhost:8099/>. `web/pkg/` is generated and gitignored.

`.github/workflows/pages.yml` does two jobs from one workflow:

- **On every PR**, it builds to wasm — purely as the type-check for the half
  `cargo test` cannot reach. No deploy.
- **On push to `main`**, it additionally uploads **all of `web/`** as the Pages
  artifact and deploys it. Anything you put in `web/` is published.

Pages must be configured with `build_type: workflow`. The concurrency group keys on
workflow + PR number or ref so a PR type-check cannot cancel an in-flight `main` deploy.

## Odds and ends

`bench_clock` (`crates/wasm/src/lib.rs`) measures deadline-check overhead in the
browser — the cost of one `Instant::now()` comparison, in the exact shape `timed_out`
uses. It has no consumer page at present; `web/clockbench.html` was a throwaway
measurement page that was accidentally published with the site and has been removed.
Read the `perf-measurement` skill before drawing conclusions from it, and get node
counts from the `bench` harness rather than hardcoding them.

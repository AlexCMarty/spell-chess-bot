---
name: perf-measurement
description: Use when measuring, optimizing, or comparing performance of the Spell Chess engine - benchmarking a search change, investigating a slowdown, deciding whether an optimization is worth it, or interpreting hotcost/qprof/bench numbers. Covers which harness answers which question, the node-count invariant every perf change must hold, and the traps that have produced hours of phantom findings here.
---

# Measuring engine performance

Every perf claim in this repo has to survive two questions: **did you time the binary you
think you timed**, and **did the search tree stay identical**. Most wrong conclusions in
this project's history failed one of those, not the algorithmics.

## The gate: node counts must be bit-identical

A perf change may not change what the search explores. Before and after, at both depths:

```sh
SPELLCHESS_PROFILE=1 cargo run --release -p spellchess-search --example bench -- 6 2>&1 | tail -3
```

That prints one `profile search:` line with `nodes=`, `qnodes=`, `spell_gens=`,
`tt_hits=`, `nmp=`, `no_spell_cut=` and `nps=`. Run it against the `bench` example, **not**
against `-p spellchess-cli`: the CLI is an interactive REPL that runs no search until a
`go` line arrives, so on EOF it prints its banner and exits with no counters at all.

If node/qnode counts move, you changed the tree, not the cost per node — that is a
different kind of change and needs correctness review, not a benchmark. This gate is
hardware-independent, which makes it the most portable check in this file.

There is also a cheap, always-on complement to the explicit counter comparison above:
`search_output_is_unchanged` in `crates/search/tests/search_identity.rs` asserts the
exact returned turn and score, for a small battery of positions at fixed depths, against
hardcoded `EXPECTED` values. It carries no `#[ignore]`, so plain `cargo test` (debug,
no env var) already runs it on every change — it will not catch every tree-shape drift
the node-count comparison would, but it catches the common case for free, with no
release build and no manual `SPELLCHESS_PROFILE` run required.

This is settled policy, not a preference: speed work must not narrow the search tree.
The widened quiescence/root behaviour is deliberate and its tactic visibility is not
traded back for speed. The canonical statement of this invariant (and of the emission-
order invariant it's downstream of) is `docs/ARCHITECTURE.md#invariants` — this section
is the procedure for checking it, not the source of truth for it.

## Which harness answers which question

| Tool | Question it answers | Command |
|---|---|---|
| `bench` | **What does the search actually cost?** The number that counts. | `cargo run --release -p spellchess-search --example bench -- 6` |
| `hotcost` | What does one call to a hot function cost? | `cargo run --release -p spellchess-search --example hotcost` |
| `qprof` | Where does that cost go, inside a node? | `--features spellchess-core/qprofile`, then `crates/search/examples/qprof.rs` |
| `SPELLCHESS_PROFILE=1` | How many nodes/qnodes/spell_gens? | env var on the `bench` example (**not** the CLI — see above) |
| `--ignored` release suite | Did I regress past a known bound? (bounds are reference-hardware seconds, scale with `SPELLCHESS_PERF_SCALE` — see below) | `cargo test -p spellchess-search --release -- --ignored` |

This runs every `#[ignore]`d test in the `spellchess-search` package, which today is
two: `depth_budget_stays_bounded_on_a_realistic_board` (the bound check above) and
`search_identity.rs`'s `print_expected`, which is not a check at all — it only prints
turns/scores for regenerating `EXPECTED` after an intentional behaviour change and
passes unconditionally. Don't read a green `print_expected` as evidence of anything.

`hotcost` says what a call costs; `qprof` says where that cost goes. **They have
disagreed.** Only `bench` settles whether the search got faster.

Run the `--ignored` suite before landing anything that touches
`generate_quiescence_from` / `generate_quiescence_turns_from`. A regression once sat on
`main` for a full commit precisely because that step was skipped.

## Protocol for comparing two commits

1. Build the baseline in a worktree at the old commit, **on the same machine**:
   `git worktree add /tmp/base <commit> && (cd /tmp/base && cargo build --release --bins --examples)`.
   The `cd` is not optional and the subshell is not decoration: without them the `&&`
   runs `cargo build` in your *current* checkout and leaves `/tmp/base` unbuilt, so the
   A/B below times the same binary twice. `--examples` is what builds `bench` and
   `hotcost`; a plain `--release` does not.
2. `SPELLCHESS_THREADS=1` for anything compared across commits. Multi-threaded timings
   vary run to run; single-threaded ones do not.
3. Interleave the A/B runs rather than doing all of A then all of B — it cancels thermal
   drift, which matters on a passively-cooled board.
4. Re-run the full `bench` at **both** depth 6 and depth 8. A change can win at one depth
   and lose at the other; this has happened.
5. Confirm node counts unchanged (above).
6. Confirm `.cargo/config.toml` is byte-identical in both worktrees, or pin `RUSTFLAGS`
   explicitly for both runs instead of relying on it. It sets `-C target-cpu=native` for
   every non-wasm target, so if the baseline worktree predates the commit that added
   that file (or a later edit to it), the two trees build with different codegen flags —
   you are then A/B-ing two compilers' output, not two algorithms, and the result is a
   codegen-gap artifact of exactly the shape the "`--examples` leaves the CLI binary
   stale" trap below already warns about.

## Traps that have each cost this project an hour

**`--examples` leaves the CLI binary stale.** `cargo build --release --workspace
--examples` restricts the build to example targets and does *not* rebuild
`target/release/spellchess`. Timing that stale binary against fresh examples once
"showed" a 20% LTO/codegen gap between them that does not exist, and it was chased with
`lto = "fat"` and `#[inline(never)]` before anyone checked the mtime. Use `--bins
--examples`, or check the mtime.

**Per-call cost is meaningless without call frequency.** A rewrite of the root generators
was nearly spec'd on their 114–201µs per-call cost — they run ~535 times in a depth-6
search, i.e. ~0.6% of runtime. Always multiply by the `SPELLCHESS_PROFILE` counters
before scoping work.

**Measure before proposing.** Incremental Zobrist and per-node `Vec` reuse were both
proposed as the big wins and measured ~2% combined. `hash_position` is 50ns, `evaluate`
86ns, `is_square_attacked` 27ns — none are bottlenecks. Run `hotcost` *first*; it is
committed for exactly this reason.

**Extracting a shared helper can silently de-inline it.** This is the subtlest one.
Pulling a closure out of `legal_moves` into a `legal_ctx`/`move_survives` pair and giving
it a *second* caller stopped LLVM inlining it into the original caller. `legal_moves` is
on far more hot paths than the call sites being optimized, so a ~60% win in the isolated
qprof buckets was almost entirely eaten by a de-inlining tax paid everywhere else —
leaving a depth-6 *regression*.

- **The tell:** a wall-clock cliff appearing on a commit that merely *adds an uncalled
  function*. That is de-inlining, not algorithmics — a non-inlined call has to actually
  move `LegalCtx`'s 520-byte `PinMap`.
- **The fix:** `#[inline(always)]` on both halves. Plain `#[inline]` was tried and was
  **not** sufficient.
- **The rule:** if a change touches a function shared with unrelated hot paths (anything
  `legal_moves`-adjacent), an isolated bucket win is not evidence. Re-run the full `bench`.

## Absolute numbers are yours to generate

Timings in this project's history were measured on a Raspberry Pi 5. **Do not treat any
absolute second-count you find in git history or an old comment as a target on your
machine.** Establish your own baseline with the worktree protocol above and work in
relative deltas. Node counts, per-call ratios, and the profile counters transfer between
machines; wall-clock seconds do not.

If your numbers look wildly off from something you read, re-baseline the unmodified
commit on your own hardware before blaming the environment. Misdiagnosing a slow run as
"different hardware" when it was a real regression has happened here.

### Calibrating `SPELLCHESS_PERF_SCALE`

The `--ignored` release suite's `depth_budget_stays_bounded_on_a_realistic_board` test
is the one exception to "don't treat an absolute second-count as a target": it has to
assert *some* wall-clock number to be a regression guard at all. It reads
`SPELLCHESS_PERF_SCALE`, a float multiplier applied to every bound in the test, default
`1.0` — the default targets the reference Raspberry Pi 5 the bounds were measured on.
To calibrate on new hardware: run the suite unmodified at scale 1.0 on an unchanged
commit, take the ratio of your wall-clock time to the reference figure in the test's doc
comment, and export `SPELLCHESS_PERF_SCALE=<that ratio>` for future runs there. Don't
edit the bounds in the test itself — that's exactly the drift this variable exists to
avoid.

## Known dead ends — do not retry these blind

**Lazy-SMP: implemented, measured a loss, defaulted off.** `search_smp` exists and works;
`default_threads()` returns 1 unless `SPELLCHESS_THREADS` overrides. Depth 6: 1 thread
6.97s, 2 threads 10.25s, 4 threads 7.4–26.5s run to run. Structurally: ~87% of nodes are
quiescence nodes and **quiescence never probes the TT**, so helper threads cannot share
that work and simply duplicate it — 2 threads did 2.6x the qnodes for 2x the cores.
Tuning thread counts or window policy will not fix this. The prerequisite is giving
quiescence something to share.

**Do not swap `jump_targets` for `relevant_jump_targets` in `generate_quiescence_from`.**
It looks like a free win by analogy with the main search generator and measured
*slower*, reproducibly (13.4s → 18.0s, confirmed twice). `relevant_jump_targets` does more
upfront work than it saves when called once per quiescence entry rather than per
recursive node. The "relevance-filtered is always cheaper" intuition does not transfer to
entry-only call sites.

**You cannot buy speed by generating fewer turns at the leaf.** Replacing the leaf
`generate_quiescence_turns_from` with the cheap recapture generator made depth 6 3.4x
faster and depth 8 *slower* (>400s vs 258s). The spell-enabled captures feed real
beta cutoffs.

These per-figure timings are attributed to the reference Pi 5 but not to a commit. The
258s and ~90–150s depth-8 figures on this page differ by roughly 2x on the same stated
hardware — almost certainly because they were measured at different points in the
project's optimization history (a `bench` run against whatever `spellchess-core` looked
like at the time), not a contradiction. Don't average or reconcile them; each is only
meaningful relative to the commit it was measured against, which none of these notes
record. Re-baseline on your own hardware and commit rather than trying to slot your
number in between these two.

## Mechanical notes

- Release builds take a couple of minutes on low-power hardware and a depth-8 `bench` run
  takes ~90–150s. Budget long timeouts rather than assuming a hang.
- `cargo` may live at `~/.cargo/bin/cargo` and not be on `PATH` in every shell.

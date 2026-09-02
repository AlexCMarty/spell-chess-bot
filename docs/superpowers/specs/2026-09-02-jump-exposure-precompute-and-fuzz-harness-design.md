# Jump-exposure precompute + coverage-guided fuzz harness — design

Date: 2026-09-02
Status: approved

## Goal

Two independent pieces of work, scoped together because the second validates the
first on the highest-risk code in the project:

1. Close item 14's open TODO 1 in `spell-chess-engine-followups` memory: `attackers_to`
   is now the floor on the jump delta path. `jump_captures`
   (`crates/core/src/spell_delta.rs:215`) calls the general `attackers_to` fresh for
   *every* jump target to detect a newly-created checker — a full pawn/knight/king/
   slider-ray scan, measured at 7.22M calls at depth 6 alone (`qprof`, item 14).
   Replace it with a per-node precompute that reduces the common case to a bitboard
   test.
2. Add a `cargo-fuzz` harness covering both the freeze and jump deltas in
   `spell_delta.rs`, including the new precompute, as a second discovery method
   alongside the hand-built-geometry + mutation-testing process that has been the
   project's only reliable way to find spell-legality bugs (see `fuzzing-is-not-
   discovery` memory: 8/8 historical bugs found by hand, 0/8 by random batteries).
   Coverage-guided fuzzing evolves inputs toward *new branches*, which is a
   structurally different search than uniform random and than hand-authored cases —
   worth having as a third leg, not a replacement for either.

## Part 1: the jump-exposure precompute

### Rule facts this depends on (both `[VERIFIED]`)

- Jump grants slider transparency only — it does not change what any piece attacks
  except by removing one square as a blocker on straight/diagonal lines. Knights,
  kings, and pawns are unaffected (`rules/40-jump.md`).
- A frozen piece "exerts no control at all" (`rules/30-freeze.md`) — occupancy is
  unaffected by freeze (freeze never changes reachability, only control), but a frozen
  slider does not attack even when unblocked.

### Why a full `attackers_to` call is wasted work per target

`checkers_after` can only ever be a **superset** of `ctx.checkers` under a jump: the
piece that was already checking (if any) still checks the same way, and the only
thing a jump can add is a new slider seeing through the jumped square. So for the vast
majority of jump targets — any square that is not the *first* blocker on one of the
king's 8 rook/bishop lines — jumping it cannot add a checker at all, and
`checkers_after` is provably `ctx.checkers` unchanged. The current code cannot see
this without calling `attackers_to` and comparing, because the check is expressed as
"recompute and see."

### The precompute

Once per node, in `NodeContext::new`, walk each of the king's 8 directions
(`ROOK_DIRS` ∪ `BISHOP_DIRS` from `rays.rs`) against `ctx.slider_occ` (which already
has *existing* live jump squares subtracted, matching how the rest of `NodeContext` is
built):

1. `first = ray_attacks(king_sq, ctx.slider_occ, dir).intersect(ctx.slider_occ)` — the
   first blocker in this direction, or empty if the ray runs off the board with no
   blocker (nothing to precompute for this direction).
2. `second = ray_attacks(first_square, ctx.slider_occ, dir).intersect(ctx.slider_occ)`
   — the next blocker past it, or empty.
3. If `second` is non-empty, the piece there is enemy-colored (`ctx.enemy`),
   **unfrozen** (`!ctx.frozen.contains(second_square)`), and its kind matches the ray
   (`Bishop`/`Queen` for a `BISHOP_DIRS` entry, `Rook`/`Queen` for `ROOK_DIRS`), then
   `first_square` is a jump-exposure square and `second_square` is the attacker it
   would reveal.

This yields at most 8 exposure squares per node (one per direction; a square lies on
at most one of the king's 8 lines, so there is no possibility of two directions
disagreeing about the same square). Store as a `Bitboard jump_exposure` plus a small
fixed array `[(Square, Square); 8]` (exposure square → revealed attacker square,
mirroring the existing `MAX_SLIDERS`-array style already used for `SliderScan`).
Color of the blocker at `first_square` is irrelevant — jump transparency doesn't care
who owns the piece being jumped, only who owns the piece revealed behind it.

### Integration into `jump_captures`

Replace the unconditional `attackers_to` call at `spell_delta.rs:215` with:

- `!ctx.jump_exposure.contains(s)` → `checkers_after` is provably `ctx.checkers`
  unchanged. Reuse `ctx.checkers` directly (no bitboard union needed, no call). The
  existing `checkers_after.count() > 1` decline becomes `ctx.checkers.count() > 1`
  (covers a pre-existing double check the caller is already in, independent of this
  jump) and `single_checker` becomes `ctx.checkers.iter().next()`.
- `ctx.jump_exposure.contains(s)` → look up the paired attacker square from the small
  array, and `checkers_after` is `ctx.checkers.with(revealed_square)` — one bitboard
  union, still no ray walk at query time. `revealed_square` cannot already be a member
  of `ctx.checkers` (its line to the king was blocked by `s`, and a square lies on at
  most one line through the king, so it cannot simultaneously be an already-open
  attacker via a different line), so `count() == ctx.checkers.count() + 1` requires no
  further reasoning.

Every other branch of `jump_captures` (the `sliders.reach` short-circuit, the pin
walk, the king-capture/pinned-onto-jump-square declines) is unchanged — this replaces
one call site's *source* of `checkers_after`, not the logic built on top of it.

### Non-goals

- The freeze path (`freeze_captures`) is untouched; it has no equivalent "recompute
  attackers per target" cost to cut.
- Root generators (`generate_search_spell_turns`, `generate_turns_from`) stay on the
  rescan path per item 13/14's standing decision — this precompute is scoped to the
  quiescence delta only.
- No attempt to precompute anything about a *pre-existing* double check beyond reusing
  `ctx.checkers` — that state is rare and already handled correctly by the existing
  decline-to-`NeedsRescan` branch.

### Testing

Hand-built fixtures (per `fuzzing-is-not-discovery` memory — do not rely on random
batteries alone for this):

- Simple rook-line exposure and simple bishop-line exposure (each direction family
  represented at least once).
- `second` present but **frozen** → must NOT be marked exposed (mutation-test this
  guard specifically: deleting the frozen check must make a named test fail).
- `second` present but wrong piece kind for the ray (e.g. a rook behind on a diagonal
  line) → must NOT be marked exposed.
- `second` present but own-colored → must NOT be marked exposed.
- `first` is our own piece vs. the enemy's piece → both must mark the exposure (color
  of the blocker doesn't matter, only the color/kind of what's behind it).
- Pre-existing double check (`ctx.checkers.count() > 1` before any jump) combined with
  a jump target both inside and outside `jump_exposure` → both must still decline via
  the `ctx.checkers.count() > 1` path.
- Ray runs off the board with zero or one blocker → no exposure entry created, no
  panic.

Mutation-test every new guard (`--no-fail-fast`) per item 13's standing process. The
existing `spell_delta_soundness.rs` differential batteries
(`check_kind_over_ordered` against `rescan_oracle`) continue to be the correctness
backstop for the integration — they exercise `jump_captures` end-to-end and do not
need to know the precompute exists.

### Measurement

Gate: node counts at depth 6 and depth 8 must stay bit-identical to the current
`main` baseline (`340473`/`2383276` at depth 6, `6453264`/`17437140` at depth 8),
same as items 13 and 14. Measure wall-clock with the committed `bench` example
(not the CLI — see `perf-measure-the-binary-you-shipped` memory), before/after, at
depth 6 and depth 8. No target number is committed to in advance; report whatever
the precompute actually gets, the way item 14 did.

## Part 2: `cargo-fuzz` harness for `spell_delta`

### Environment note

This machine only had the `stable` rustup toolchain and no reachable `crates.io`
inside a Claude Code session at the start of this work; both are prerequisites for
`cargo-fuzz` (nightly + `libfuzzer-sys`/`arbitrary` fetched fresh). The user installed
`rustup toolchain install nightly` and `cargo install cargo-fuzz` from their own
terminal (outside the agent session) and confirmed `cargo-fuzz 0.13.2` plus a full
`cargo +nightly fuzz init && cargo +nightly fuzz build` smoke test succeeded from
within the agent session too — the earlier 403s were this agent session's own egress
policy, already resolved, not a property of the host or the sandbox generally.

### Layout

`crates/core/fuzz/`, created via `cargo +nightly fuzz init` (standard cargo-fuzz
layout: its own `Cargo.toml` with `libfuzzer-sys` as a dependency and a path
dependency on `spellchess-core`, excluded from the main workspace the way cargo-fuzz
crates conventionally are). This keeps `spellchess-core`'s own `Cargo.toml`
dependency-free in non-dev builds, matching the existing policy that added
`serde`/`serde_json` as `[dev-dependencies]` only for `oracle_vectors.rs`.

Two fuzz targets, `fuzz_freeze` and `fuzz_jump`:

- Each target decodes the raw fuzzer bytes into a `Position` using a byte-driven
  version of the existing `sparse_random_positions`/`king_tangle_positions`
  generators in `spell_delta_soundness.rs` (same piece-placement/field-placement
  approach, just consuming fuzzer-supplied bytes for each random choice instead of a
  `splitmix64` seed) — one shared mental model for "how adversarial positions get
  built" across hand-written and fuzzed tests, and no new dependency
  (`arbitrary`-derive) needed even though it's transitively available.
- For every castable target of the relevant `SpellKind` on the decoded position, both
  targets assert `captures_enabled_by` agrees with `rescan_oracle` in *set and order*
  whenever it returns `Delta::Complete` — literally `check_kind_over_ordered`'s
  comparison, reused rather than reimplemented, so a libFuzzer failure and a `cargo
  test` failure mean the same thing and are debugged the same way.

This exercises the Part 1 precompute for free, since it lives inside `jump_captures`
with no separate call path.

### Regression policy

The fuzz corpus is a discovery tool, not a regression gate — matching how this
project has always worked (item 13: every bug found by hand-built geometry or
mutation testing became a permanent named test). Any crash or mismatch gets minimized
with `cargo fuzz tmin` and turned into a hand-authored case added to
`spell_delta_soundness.rs`, with the same "assert the slow oracle *does* contain the
move" discipline `fuzzing-is-not-discovery` calls out for hand-built cases, so the
committed test suite — not the corpus — is what CI and future sessions rely on.

`crates/core/fuzz/{Cargo.toml,fuzz_targets/}` are committed. `crates/core/fuzz/
{corpus,artifacts,target}/` are gitignored, same treatment as any other build output.

### Non-goals

- No CI wiring for continuous fuzzing — this is a local discovery tool run
  interactively, at least for now. If the project wants scheduled fuzzing later,
  that's a separate, smaller follow-up once this harness exists and has a track
  record.
- Not fuzzing anything outside `spell_delta.rs` (no root generator fuzzing, no
  general `legal_moves` fuzzing) — scoped to the two functions this session's
  precompute work touches, per the user's explicit "cover both" scope decision.

## Workflow

New branch off `main` (`1b93e07` at time of writing). This spec is written and
committed before implementation begins, per the architectural brainstorming path.
Single merge at the end, matching items 13 and 14's pattern (`--no-ff`, feature
branch deleted after).

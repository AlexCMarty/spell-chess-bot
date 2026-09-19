---
name: spell-legality-testing
description: Use when writing, changing, or testing freeze/jump legality logic in Spell Chess - spell_delta.rs, legal.rs, movegen.rs, captures_enabled_by, the quiescence spell generators, or any fast path that claims to match the slow rescan oracle. Explains why random fuzzing does not find bugs in this codebase, what does, and the recurring bug shapes to check for.
---

# Testing spell legality

Spell interactions in this variant are subtler than they look on paper. That claim is not
folklore — it is the single most repeated finding in this project's history, and it has
been re-learned at least four separate times. Budget accordingly.

## Random fuzzing is a regression net, not a discovery tool

Across the spell-capture delta work, **eight separate unsoundness bugs shipped into
planned, reviewed code. Not one was found by a random battery.** Every one came from a
hand-built adversarial geometry or from mutation-testing an individual guard. Four of
nine mutants were killed *only* by hand-written fixtures. A later item shipped two more
bugs and the pattern held exactly.

The reason is structural: random legal walks from the starting position essentially never
produce the geometries these bugs live in — two simultaneous jump fields, a pin created by
a *field* rather than a piece, en passant combined with a freeze, your own king frozen by
an *older opponent* field. The state space that matters is a thin shell that uniform
sampling misses. One real bug (emission order) occurred about once per 10,000 random
positions, so even a 512,000-position sweep was a coin flip.

**A green battery proves nothing about correctness here.** Treat it as regression
protection for things you already fixed.

### Targeted generators are not "random batteries"

`ray_dense_positions`, `sparse_random_positions`, and `king_tangle_positions` are seeded
by a PRNG but bias placement toward pins, x-rays, and king-adjacency. These *have* caught
real bugs. They are engineered adversarial geometry with a random seed — the opposite of
a uniform walk from the start position. Prefer them, and add new ones when you model a
new mechanism.

### cargo-fuzz is unproven here, not endorsed

`crates/core/fuzz/` (`fuzz_freeze`, `fuzz_jump`) has had one short trial — roughly 560k
execs across both targets — and found nothing, while the existing hand-built battery
found two real bugs in the same work. That is too short to conclude anything either way.
Until there is a real data point on that tool specifically, do not treat a clean fuzz run
as evidence. Note that the fuzz crate carries its own `[workspace]` table and is **not**
covered by `cargo build|test --workspace`.

**Neither prerequisite is installed by default** — a fresh environment here has stable
only and no `cargo-fuzz` binary, so the command below fails as written until you run
`rustup toolchain install nightly` and `cargo install cargo-fuzz`:

```sh
cd crates/core && cargo +nightly fuzz run fuzz_jump -- -max_total_time=120
```

Because that crate is outside the workspace, **run `cargo +nightly fuzz build` by hand
after any change to `spellchess-core`'s public API** the targets depend on
(`captures_enabled_by`, `Delta`, `spells::jump_targets`/`freeze_targets`). Nothing else
will catch a break there. The same now goes for `crates/wasm`, which also consumes that
API and is only compiled by `wasm-pack build crates/wasm --target web` (CI runs it per PR).

## What to actually do

**1. Hand-build the adversarial position for every mechanism you model.**

Assert the slow oracle *contains* the move before asserting what the fast path does.
Otherwise the test passes vacuously — both sides agree the move isn't there, and you have
verified nothing. This is the most common way a spell fixture is silently useless.

**2. Mutation-test every guard you write.**

Disable the guard, confirm a *named* test dies. A guard no test can kill is either dead
code or untested — find out which and say so.

Mutation testing here is **manual**. There is no `cargo-mutants`, no `mutants.toml`, and
nothing to install — you edit the guard out by hand, run the tests, and put it back.

```sh
# fast inner loop, one target at a time
cargo test -p spellchess-core --no-fail-fast <test_name>
# confirmation run before you call a guard covered
cargo test --workspace --no-fail-fast
# the batteries cargo test never runs on its own (see the table below)
cargo test -p spellchess-core --release -- --ignored
```

`--no-fail-fast` is mandatory for mutation work: plain `cargo test` stops at the first
failing target and will **mis-attribute kills**.

**Where the batteries live.** The reasoning above is about these files:

| Path | What is in it |
|---|---|
| `crates/core/tests/spell_delta_soundness.rs` | the main fast-path-vs-oracle battery: freeze/jump × battery / king_tangle / sparse / cramped / ray_dense |
| `crates/core/tests/relevance_soundness.rs` | relevance filtering vs. an exhaustive target sweep |
| `crates/core/tests/legal_moves_soundness.rs` | legality soundness — plus an **`#[ignore]`d** dense random-walk battery near the end of the file |
| `crates/core/tests/legal_captures.rs` | hand-derived expected output for the captures fast path |
| `crates/core/tests/oracle_vectors.rs` | loads `crates/core/tests/fixtures/*.json`, asserts at least 20 |
| `crates/core/src/spell_delta.rs`, `movegen.rs` | white-box `mod tests`, including the ordered-equivalence property tests |

The `#[ignore]`d battery is the one to remember: `cargo test --workspace` **never** runs
it, so a change that only it would catch looks green. Run the `--ignored` line above
before landing anything that touches the quiescence generators.

Watch for fixtures that pass for the wrong reason. One fixture covering a walk-through-
transparency loop only ever needed a single iteration to pass, so a mutation deleting the
loop's continuation entirely still passed. If your guard has a loop, build a fixture that
forces at least two iterations.

**3. Keep the slow implementation permanently as the differential oracle.**

Never delete the rescan path when a fast path lands. The `Delta::NeedsRescan` escape hatch
— **decline instead of guessing** — is what made shipping an incomplete fast path safe.
When the fast path isn't sure, it should say so and fall back, not approximate.

**4. Emission order is part of the contract, not just the set.**

`generate_quiescence_from` pushes turns unsorted and the consumers read them unsorted, so
a generator returning the right set in the wrong sequence changes beta-cutoff order and
qnode counts. Use `check_kind_over_ordered`, which distinguishes `SET MISMATCH` from
`ORDER MISMATCH`. A best-move-and-score identity test structurally **cannot** catch this.

`pseudo_legal_moves` is **not** sorted by `(from, to)`: it emits **castling first**
(`out.extend(castle_moves(..))` before the per-square loop), then each own non-frozen
square in ascending square index, and within a square in fixed per-`PieceKind` order.
So `e1`'s castle precedes `a1`'s rook moves, and a pawn emits its double push before its
captures. `pseudo_legal_captures` mirrors that relative order exactly. A replacement must
reproduce this order, not a sorted one — see the Invariants section of
`docs/ARCHITECTURE.md`, which is canonical for it.

This is the same invariant as the `/perf-measurement` skill's "node counts must stay
bit-identical", seen from the generator side: change emission order and the node counts
move, which that skill will read as a correctness regression rather than a speed win.

## Recurring bug shapes — check these by name

**`zone` where `frozen_after` was meant.** Your own pieces frozen by an *older opponent*
field also have zero legal moves. The current cast's zone is not the full frozen set.

**Forgetting the pin branch's clone-and-rescan.** `legal_moves`' pin handling resolves
"capture onto a live jump square" by cloning and rescanning against the hypothetical
position. A fast path that reasons about pins statically will get this wrong.

**Stacked transparency.** A piece standing on an *already*-transparent square still
attacks from its own square — transparency only means it does not block *other* rays
passing through it. Separate "does this square block" from "is there an attacker here",
and walk *through* already-transparent occupied squares rather than stopping at them. One
blocker can reveal more than one attacker.

**Jump is additive, not substitutive.** You can capture the piece standing **on** the jump
square, not just slide through it. This is `[VERIFIED]` in `rules/40-jump.md` and it has
now broken hand-derived fixture expectations at least twice. When a hand-written expected
output disagrees with the oracle about a jump square, suspect the fixture first.

## Before you trust your own reasoning

Read `rules/INDEX.md` — specifically that frozen pieces exert **no control at all**, that
a jump field belongs to the square and serves **both** players, and that the king can be
legally captured. `rules/70-engine-api.md` has a harness for driving chess.com's own
engine as a ground-truth oracle; `rules/90-test-vectors.md` has 20 engine-verified
entries — but vector 20 is an **anti-vector** ("API permissiveness — do NOT treat as legal
play"): it records turns the raw engine accepts that are *not* legal Spell Chess. Do not
lift it into a fixture as expected behaviour.

Prefer the oracle and adversarial fixtures over hand-derived reasoning. Every time this
project has trusted a careful manual argument about freeze/jump interaction, it has been
wrong.

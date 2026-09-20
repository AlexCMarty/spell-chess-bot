---
name: oracle-verification
description: Use when verifying a rule in rules/ against chess.com's real engine, adding or regenerating a fixture under crates/core/tests/fixtures/*.json, running or debugging crates/core/tests/oracle_vectors.rs, deciding what confidence tag ([VERIFIED]/[CODE]/[DOC]/[UNVERIFIED]) a new rule statement earns, fetching or re-pinning the variants.js engine bundle, or opening any chess.com URL from this repo. Covers guard-chesscom-url.py's actual coverage (and its gaps), the fixture JSON schema (documented nowhere in prose except here), the engine-version-check.yml staleness trap, and the rules/90-test-vectors.md anti-vector that must never become a fixture.
---

# Verifying rules against the real engine

## The constraint that comes before everything else

**Only ever open `https://www.chess.com/variants/spell-chess/analysis`.** Every other
chess.com page can match this client into a live game against a human — a terms-of-service
violation that risks the account. The one other allowed path is
`/r2/client-packages/variants/<version>/variants.js` — an inert static asset, not a page
that can seat you against anyone, which is why `.github/workflows/engine-version-check.yml`
is allowed to `curl` it in CI.

`.claude/hooks/guard-chesscom-url.py` enforces this as a `PreToolUse` hook. **If it blocks
you, it is right — do not work around it.** But read it before assuming it is a wall:

- On `Bash`, it only inspects the command at all if it matches `FETCH_HINT` (`curl`,
  `wget`, `xdg-open`, a browser name, `playwright`, `puppeteer`, `selenium`, `httpie`,
  `lynx`, `w3m`, `webbrowser`, …). A command that reaches chess.com some other way is not
  even scanned.
- On other tools it only looks at the values of `url`/`uri`/`href`/`link` fields, or any
  string field that literally contains `"chess.com"`.
- `WebSearch` is not in `.claude/settings.json`'s hook matcher (`WebFetch|Bash|mcp__.*
  (chrome|browser|playwright|puppeteer).*`) at all.
- It does not follow redirects, and any unhandled exception inside it exits `0` (allow) —
  a broken guard must never wedge the session.

So the hook is a speed bump against an obvious mistake, not a guarantee. The discipline —
never navigate anywhere but `/analysis`, never chase a redirect off it — is yours, not the
hook's. To convince yourself of its actual coverage rather than trusting this summary, run
`bash .claude/hooks/test-guard-chesscom-url.sh`.

## Getting the engine bundle

`research/` is gitignored; the bundle is third-party copyrighted code and is never
committed or redistributed. Fetch your own copy (README.md's procedure):

```sh
mkdir -p research && cd research
curl -O https://www.chess.com/r2/client-packages/variants/2026.8.1/variants.js
```

That request is a plain `curl` to a `/r2/client-packages/` path, which the guard allows.
The version string moves — chess.com can reship a new bundle under a new version, or the
same one, at any time. If the URL 404s, get the current version from the analysis page's
network tab (that page only) and update the pin everywhere it's recorded (below) before
re-deriving anything from it.

## Running the harness

`rules/70-engine-api.md` has the full copy-pasteable JS: reaching the `fpc` engine
instance on the Pinia store, the 14×14 coordinate helpers, the sandbox factory (never call
`.reset()` on the live instance), the FEN builder, and `dests`/`spellTargets`/`turn`
helpers for querying and playing. Don't re-derive any of that here — read it there.

## The fixture schema

This is the piece that exists nowhere in prose except this file — everywhere else it's
implicit in the `Deserialize` structs in `crates/core/tests/oracle_vectors.rs`. A fixture
is one JSON file under `crates/core/tests/fixtures/`, shaped like
`crates/core/tests/fixtures/field_freeze_00_center.json`:

```json
{
  "name": "field_freeze_00_center",
  "pieces": { "e1": "0K", "e8": "2K", "d5": "2R" },
  "side_to_move": "black",
  "castle_rights": {
    "white_kingside": false, "white_queenside": false,
    "black_kingside": false, "black_queenside": false
  },
  "white_spells": { "freeze": {"count": 4, "lock": 3}, "jump": {"count": 2, "lock": 0} },
  "black_spells": { "freeze": {"count": 5, "lock": 0}, "jump": {"count": 2, "lock": 0} },
  "fields": [ {"square": "d5", "owner": "white", "kind": "freeze", "expires_after_ply": 1} ],
  "ply": 1,
  "expected": {
    "destinations": { "e8": ["d7", "d8", "e7", "f7", "f8"] },
    "freeze_targets": ["a1", "a2", "…"],
    "jump_targets": ["a2", "d5", "e1", "e8", "h7"]
  }
}
```

Field notes:

- `pieces` keys are 8×8 algebraic squares; a piece code is 2 bytes: `'0'`/`'2'` for
  white/black (matching the engine's own player-index convention, not `'w'`/`'b'`), then
  the uppercase piece letter (`K`,`Q`,`R`,`B`,`N`,`P`).
- `expected.destinations` need only list squares whose piece belongs to `side_to_move` —
  the test iterates the board and defaults any square it can't find in the map to an empty
  set, so an omitted square asserts "no legal moves for that piece," not "untested."
- `expected.freeze_targets` / `jump_targets` must come from the **engine's** exhaustive
  target lists (via `spellTargets(g, 'freeze'|'jump')` in the harness), not from
  `spellchess_core::spells::freeze_targets`/`jump_targets` directly — the test compares
  against `generate_turns`' output, which additionally drops any cast that would leave the
  mover with zero completing moves. The raw `spells::` functions don't model that filter.
  See the comment above `check_fixture` in `oracle_vectors.rs` and the sparse-position
  fixtures it points at.
- There's no `en_passant` field — `build_position` in `oracle_vectors.rs` always sets it to
  `None`. A fixture that needs to test en passant isn't expressible in this schema as it
  stands.

Naming convention in `crates/core/tests/fixtures/`: `dense_*` and `medium_*` are
busier boards, `sparse_*` are few-piece endgame-shaped positions, `field_freeze_*` /
`field_jump_*` isolate one live spell field's interaction (blocker, discovered-attack,
self-effect, corner clipping, own-piece freezing). Match an existing family when adding
a fixture for the same kind of geometry; start a new family name when you aren't.

## The validation loop

```sh
cargo test -p spellchess-core --test oracle_vectors
```

`oracle_fixtures_match_engine_output` walks every `*.json` under
`crates/core/tests/fixtures/`, rebuilds a `Position` from it, and asserts
`legal_moves`/`generate_turns`'s output against `expected`. It also asserts at least 20
fixtures were found — deleting one without replacing it fails the count, not just the
content. A failure names the fixture and the exact square or target-list mismatch; treat a
failure here as a real behavioural regression (or a stale fixture — see below), never as
noise to relax an assertion around.

## The staleness trap

Every `[VERIFIED]` tag in `rules/` means "observed in bundle `variants/2026.8.1`," not
"true forever." That bundle is live third-party code chess.com can change without notice.
If freeze/jump/anything semantics move in a future bundle, **every tag built on the old
behaviour goes stale silently**: `oracle_vectors.rs` was generated from the old bundle, so
it keeps passing and keeps confirming the old, now-wrong behaviour. There is no test that
can catch this on its own — it needs a human or agent to re-run the harness against a
fresh bundle and diff.

`.github/workflows/engine-version-check.yml` only checks that the pinned URL still
resolves (HTTP 200) once a week. It does **not** check that the bundle at that URL still
behaves the same way — a chess.com deploy that changes semantics under the *same* version
string slips straight past it. Treat that workflow as a "the pin fell off a cliff" alarm,
never as evidence the ruleset is current.

## Confidence tags

- `[VERIFIED]` — produced by actually executing the real engine (the harness above) and
  observing its output.
- `[CODE]` — read directly out of `variants.js`, not separately executed.
- `[DOC]` — chess.com's own help-centre/in-client prose. Known to contain at least three
  outright errors (`rules/00-overview.md#known-errors-in-public-sources`).
- `[UNVERIFIED]` — an inference nobody has checked yet. Treat as a hypothesis.

**Code wins when code and prose disagree.** A rule statement you add without running it
through the harness earns `[UNVERIFIED]` at best, `[CODE]` if you can point at the exact
`variants.js` logic, never `[VERIFIED]`.

## The anti-vector trap

`rules/90-test-vectors.md` vector 20 ("API permissiveness") records two things the raw
engine `play()` call accepts that are **not** legal Spell Chess: a `multimoves` array
containing two spells, and a bare spell cast with no following move. The one-spell-plus-
mandatory-move structure is enforced by the client UI, not by `play()`. If you ever
harvest fixtures by scripting `play()` calls, **never** let vector 20's shape (or anything
like it) end up asserted as expected legal output in a `crates/core/tests/fixtures/*.json`
file — it would bake API permissiveness into the oracle as if it were a rule.

## Where the pin lives

The version string `2026.8.1` is recorded in five places that must move together:
`rules/INDEX.md` (context header and Provenance section), `rules/70-engine-api.md`,
`rules/90-test-vectors.md`, `README.md` (the `curl` line), and the `VERSION=` line in
`.github/workflows/engine-version-check.yml`. `grep -rn '2026\.8\.1'` from the repo root
before shipping a re-pin, to make sure none of the five was missed — the workflow's own
comment says to keep it in sync with `rules/INDEX.md`. After a re-pin, re-run every
`[VERIFIED]` claim you can afford to and regenerate any fixture the new bundle disagrees
with; see `.github/workflows/engine-version-check.yml`'s auto-filed issue body for the
full step list it expects a human/agent to follow.

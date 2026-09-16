// The view. Holds no authoritative game state -- `state` below is only the last
// snapshot the worker sent, and every mutation goes back through the worker.
// A staged spell is the one exception: it is a *pending* cast that has not been
// applied yet, so it lives here until the move it owes is played.
import { startPrimer } from "./primer.js";

const worker = new Worker("./worker.js", { type: "module" });

let nextId = 1;
const pending = new Map();
let onProgress = () => {};

worker.onmessage = (event) => {
  const msg = event.data;
  if (msg.type === "progress") {
    onProgress(msg);
    return;
  }
  const entry = pending.get(msg.id);
  if (!entry) return;
  pending.delete(msg.id);
  if (msg.ok) entry.resolve(msg.data);
  else entry.reject(new Error(msg.error));
};

worker.onerror = (event) => {
  showError(`Engine failed to load: ${event.message ?? "unknown error"}`);
};

function call(cmd, args = {}) {
  const id = nextId++;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    worker.postMessage({ id, cmd, ...args });
  });
}

const $ = (id) => document.getElementById(id);

function showError(text) {
  const el = $("error");
  el.textContent = text;
  el.hidden = false;
}

function clearError() {
  $("error").hidden = true;
}

const GLYPHS = {
  wK: "♔", wQ: "♕", wR: "♖", wB: "♗", wN: "♘", wP: "♙",
  bK: "♚", bQ: "♛", bR: "♜", bB: "♝", bN: "♞", bP: "♟",
};

const NAMES = { K: "king", Q: "queen", R: "rook", B: "bishop", N: "knight", P: "pawn" };

const FILES = "abcdefgh";
const squareName = (index) => FILES[index % 8] + (Math.floor(index / 8) + 1);
const squareIndex = (name) => (Number(name[1]) - 1) * 8 + FILES.indexOf(name[0]);

/// The 3×3 block a freeze immobilises, clipped to the board -- mirrors the
/// engine's `freeze_zone` so the tint matches what is actually frozen.
function freezeZone(square) {
  const file = FILES.indexOf(square[0]);
  const rank = Number(square[1]) - 1;
  const out = [];
  for (let df = -1; df <= 1; df++) {
    for (let dr = -1; dr <= 1; dr++) {
      const f = file + df, r = rank + dr;
      if (f >= 0 && f < 8 && r >= 0 && r < 8) out.push(FILES[f] + (r + 1));
    }
  }
  return out;
}

let state = null;
let legalTurns = [];
let selected = null;      // square name the user picked a piece on
let staged = null;        // {kind, at} spell cast this turn, move still owed
let thinking = false;     // blocks input while the engine searches
let armed = null;         // spell kind whose targets are being shown, or null
let hover = null;         // hovered square, for the Freeze zone preview
let focused = "e1";       // roving tabindex: the one square in the tab order

// Depth, evaluation and best turn, updated per completed iteration. No principal
// variation: the engine tracks a best move, not a line, and reconstructing one
// means walking the transposition table.
let analysis = [];

const moveLog = [];

const motionless = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/// Scores come back from the side to move's point of view. The panel reports
/// from White's, which is the convention every chess UI uses, so a reader is not
/// re-orienting the sign every ply.
function evalText(score, sideToMove) {
  const white = sideToMove === "white" ? score : -score;
  if (Math.abs(white) >= 99000) {
    const plies = 100000 - Math.abs(white);
    return `${white > 0 ? "+" : "-"}M${Math.ceil(plies / 2)}`;
  }
  return `${white >= 0 ? "+" : ""}${(white / 100).toFixed(2)}`;
}

function sameSpell(a, b) {
  if (a === null && b === null) return true;
  if (a === null || b === null) return false;
  return a.kind === b.kind && a.at === b.at;
}

/// Turns available given whatever spell is currently staged. Staging changes
/// what is legal, so every question about moves has to be asked through it.
const turnsForStaged = () => legalTurns.filter((t) => sameSpell(t.spell, staged));

const destinationsFrom = (square) =>
  turnsForStaged().filter((t) => t.from === square).map((t) => t.to);

/// Squares this spell may legally target, taken from the legal turn list rather
/// than re-derived from the rules -- the engine already knows.
function spellTargets(kind) {
  const seen = new Set();
  for (const t of legalTurns) {
    if (t.spell && t.spell.kind === kind) seen.add(t.spell.at);
  }
  return seen;
}

const inProgress = () => state && state.status.kind === "inProgress";
const myTurn = () => inProgress() && !thinking && state.sideToMove === "white";

/// Which squares a live field currently affects. A staged cast is included:
/// its field is live from the moment it is cast, even though the turn it
/// belongs to has not been applied yet.
function activeFields() {
  const frozen = new Set();
  const jumped = new Set();
  const fields = state.fields.slice();
  if (staged) fields.push({ kind: staged.kind, square: staged.at });
  for (const f of fields) {
    if (f.kind === "jump") jumped.add(f.square);
    else for (const sq of freezeZone(f.square)) frozen.add(sq);
  }
  return { frozen, jumped };
}

// ── board ────────────────────────────────────────────────────────────────────

const squares = new Map();   // square name -> button element

/// Built once, then only mutated. Rebuilding all 64 nodes per render would drop
/// keyboard focus mid-game.
function buildBoard() {
  const board = $("board");
  // Rank 8 first so the DOM reads top-to-bottom the way the board looks.
  for (let rank = 7; rank >= 0; rank--) {
    const row = document.createElement("div");
    row.className = "board-row";
    row.setAttribute("role", "row");
    for (let file = 0; file < 8; file++) {
      const name = squareName(rank * 8 + file);
      const sq = document.createElement("button");
      sq.type = "button";
      sq.className = `sq ${(rank + file) % 2 === 0 ? "dark" : "light"}`;
      sq.dataset.square = name;
      sq.setAttribute("role", "gridcell");
      sq.tabIndex = -1;

      const piece = document.createElement("span");
      piece.className = "piece";
      const dot = document.createElement("span");
      dot.className = "dot";
      const ring = document.createElement("span");
      ring.className = "ring";
      const coord = document.createElement("span");
      coord.className = "coord";
      // File letters along rank 1, rank numbers up file a; a1 carries both.
      coord.textContent = (rank === 0 ? FILES[file] : "") + (file === 0 ? rank + 1 : "");

      sq.append(piece, dot, ring, coord);
      row.append(sq);
      squares.set(name, sq);
    }
    board.append(row);
  }
}

function describeSquare(name, code, frozen, jumped, isDestination) {
  const what = code ? `${code[0] === "w" ? "white" : "black"} ${NAMES[code[1]]}` : "empty";
  const notes = [];
  if (frozen) notes.push("frozen");
  if (jumped) notes.push("transparent");
  if (isDestination) notes.push("legal move");
  return `${name}, ${what}${notes.length ? `, ${notes.join(", ")}` : ""}`;
}

function renderBoard() {
  const destinations = new Set(selected ? destinationsFrom(selected) : []);
  const { frozen, jumped } = activeFields();
  const armedTargets = armed ? spellTargets(armed) : new Set();
  const preview = armed === "freeze" && hover && armedTargets.has(hover)
    ? new Set(freezeZone(hover))
    : new Set();

  for (const [name, sq] of squares) {
    const code = state.board[squareIndex(name)];
    const isDestination = selected !== null && destinations.has(name);

    sq.classList.toggle("preview", preview.has(name));
    sq.classList.toggle("frozen", frozen.has(name));
    sq.classList.toggle("jumped", jumped.has(name));
    sq.classList.toggle("selected", selected === name);
    sq.classList.toggle("staged-freeze", staged !== null && staged.kind === "freeze" && staged.at === name);
    sq.classList.toggle("staged-jump", staged !== null && staged.kind === "jump" && staged.at === name);

    const piece = sq.children[0];
    piece.textContent = code ? GLYPHS[code] : "";
    piece.className = `piece ${code && code[0] === "w" ? "white" : "black"}`;

    sq.children[1].hidden = !isDestination;
    // Jump's targeting affordance: a dashed ring on every square it may legally
    // make transparent, which is what tells a newcomer it wants a *piece*.
    sq.children[2].hidden = !(armed === "jump" && armedTargets.has(name));

    sq.tabIndex = name === focused ? 0 : -1;
    sq.setAttribute("aria-label", describeSquare(name, code, frozen.has(name), jumped.has(name), isDestination));
  }
}

// ── the rest of the stage ────────────────────────────────────────────────────

function gameOverText() {
  const s = state.status;
  if (s.kind === "kingCaptured") {
    return s.winner === "white" ? "You win — king captured." : "You lose — the king fell.";
  }
  if (s.kind === "checkmate") {
    return s.winner === "white" ? "You win — checkmate." : "You lose — checkmate.";
  }
  if (s.kind === "stalemate") return "Stalemate — a draw.";
  return null;
}

function renderStatus() {
  const over = gameOverText();
  $("status").textContent = over
    ?? (thinking ? "Engine thinking…"
      : state.sideToMove === "white" ? "Your move" : "Black to move");

  const { frozen, jumped } = activeFields();
  let hint = spellReadiness();
  if (armed === "freeze") hint = "pick a square to freeze";
  else if (armed === "jump") hint = "pick a piece to see through";
  else if (staged) hint = `${staged.kind} cast — now make your move`;
  else if (frozen.size) hint = "a freeze field is live";
  else if (jumped.size) hint = "a square is transparent";
  $("hint").textContent = over ? "" : hint;
}

// Never assert "spells ready" without checking. A spell is castable only when it
// has charges left AND its cooldown has run out; saying otherwise is the single
// most misleading thing this status line can do.
function spellReadiness() {
  const ready = [];
  const waiting = [];
  let spent = 0;
  for (const kind of ["freeze", "jump"]) {
    const { count, lock } = state.spells.white[kind];
    if (count === 0) spent++;
    else if (lock > 0) waiting.push(`${kind} in ${lock}`);
    else ready.push(kind);
  }
  if (ready.length === 2) return "both spells ready";
  if (ready.length === 1) return `${ready[0]} ready`;
  if (waiting.length) return `on cooldown — ${waiting.join(", ")}`;
  return spent ? "no spells left" : "no spell available";
}

function renderCoach() {
  const { frozen } = activeFields();
  let text;
  if (armed === "freeze") {
    text = "Freeze locks a 3×3 block until your opponent's next turn ends — your own pieces included, and it lands before your move. Tap the centre square.";
  } else if (armed === "jump") {
    text = "Jump makes one occupied square transparent to sliding pieces, for both sides. Tap the piece to see through.";
  } else if (staged) {
    text = "The spell is cast and its field is live. You still owe a piece move this turn — or cancel to take the spell back.";
  } else if (!inProgress()) {
    text = "Start a new game whenever you like. Undo takes back your last move and the engine's reply.";
  } else if (frozen.size) {
    text = "Anything inside the field can't move, and controls nothing while frozen — no check, no defence, no pins.";
  } else {
    text = "Tap one of your pieces to see where it can go. Cast a spell first if you want to change what's possible.";
  }
  $("coach").textContent = text;
}

const CHARGES = { freeze: 5, jump: 2 };

function renderDock() {
  for (const kind of ["freeze", "jump"]) {
    const btn = $(`cast-${kind}`);
    const counter = state.spells.white[kind];
    // A staged cast has spent its charge even though the turn has not landed.
    const left = counter.count - (staged && staged.kind === kind ? 1 : 0);

    btn.disabled = !myTurn() || staged !== null || spellTargets(kind).size === 0;
    btn.setAttribute("aria-pressed", String(armed === kind));
    // Show the count even while locked: the pips render it regardless, so
    // replacing the label with the cooldown made the two widgets disagree.
    btn.querySelector("[data-sub]").textContent =
      counter.lock > 0
        ? `${left} left · ready in ${counter.lock} ${counter.lock === 1 ? "turn" : "turns"}`
        : `${left} left`;
    btn.title =
      `${CHARGES[kind]} per game, never replenished. After casting, ` +
      `wait 3 of your own turns before casting ${kind} again.`;

    const pips = btn.querySelector("[data-pips]");
    pips.replaceChildren();
    for (let i = 0; i < CHARGES[kind]; i++) {
      const pip = document.createElement("i");
      if (i < left) pip.className = "on";
      pips.append(pip);
    }
  }

  $("cancel-spell").hidden = staged === null;
  $("undo").disabled = thinking || moveLog.length === 0;
  $("analyze").disabled = thinking || !inProgress();
  $("millis").disabled = thinking;
}

function renderMoves() {
  const el = $("moves");
  el.replaceChildren();
  for (const text of moveLog) {
    const li = document.createElement("li");
    li.textContent = text;
    el.append(li);
  }
  el.scrollTop = el.scrollHeight;
  $("move-count").textContent = `${moveLog.length} played`;
  $("moves-empty").hidden = moveLog.length > 0;
}

function renderAnalysis() {
  const el = $("analysis");
  el.replaceChildren();
  for (const line of analysis.slice().reverse()) {
    const tr = document.createElement("tr");
    for (const cell of [`d${line.depth}`, line.eval, line.text]) {
      const td = document.createElement("td");
      td.textContent = cell;
      tr.append(td);
    }
    el.append(tr);
  }
  $("analysis-depth").textContent = analysis.length
    ? `depth ${analysis[analysis.length - 1].depth}`
    : "";
}

function render() {
  renderBoard();
  renderStatus();
  renderCoach();
  renderDock();
  renderMoves();
  renderAnalysis();
}

// ── move notation ────────────────────────────────────────────────────────────

/// `[<spell>@<square> ]<PieceLetter><from><separator><to>` -- the engine's own
/// `format_turn` emits bare coordinates, which reads poorly in a log a learner
/// is meant to follow, so the piece letter and capture marker are added here
/// from the position the turn was played in.
function describeTurn(before, turn) {
  const code = before.board[squareIndex(turn.from)];
  const letter = code && code[1] !== "P" ? code[1] : "";
  const target = before.board[squareIndex(turn.to)];
  // A pawn changing file onto an empty square is an en-passant capture.
  const enPassant = code && code[1] === "P" && turn.from[0] !== turn.to[0] && !target;
  const sep = target || enPassant ? "x" : "-";
  const promo = turn.promo ? `=${turn.promo.toUpperCase()}` : "";
  const spell = turn.spell ? `${turn.spell.kind}@${turn.spell.at} ` : "";
  return `${spell}${letter}${turn.from}${sep}${turn.to}${promo}`;
}

// ── cast burst ───────────────────────────────────────────────────────────────

/// Pure decoration: 16 dots thrown out of the cast square. Dropped entirely
/// under `prefers-reduced-motion`.
function playBurst(square, kind) {
  if (motionless) return;
  const el = $("burst");
  const sq = squares.get(square);
  const box = sq.getBoundingClientRect();
  const wrap = $("board").parentElement.getBoundingClientRect();
  el.style.left = `${box.left - wrap.left + box.width / 2}px`;
  el.style.top = `${box.top - wrap.top + box.height / 2}px`;
  el.replaceChildren();

  const colour = kind === "freeze" ? "#bcd8ee" : "#ffc6a5";
  for (let i = 0; i < 16; i++) {
    const angle = (i / 16) * Math.PI * 2;
    const distance = 52 + Math.random() * 40;
    const size = i % 2 === 0 ? 7 : 11;
    const dot = document.createElement("span");
    dot.style.width = `${size}px`;
    dot.style.height = `${size}px`;
    dot.style.marginLeft = `${-size / 2}px`;
    dot.style.marginTop = `${-size / 2}px`;
    dot.style.background = colour;
    dot.style.boxShadow = `0 0 12px 3px ${colour}`;
    dot.style.setProperty("--tx", `${Math.cos(angle) * distance}px`);
    dot.style.setProperty("--ty", `${Math.sin(angle) * distance}px`);
    el.append(dot);
  }
  el.hidden = false;
  clearTimeout(playBurst.timer);
  playBurst.timer = setTimeout(() => { el.hidden = true; el.replaceChildren(); }, 900);
}

// ── interaction ──────────────────────────────────────────────────────────────

function onSquareClick(name) {
  if (!myTurn()) return;

  // Arming a spell: the click picks its target, and destinations are recomputed
  // for that cast -- moves illegal a moment ago may now be legal.
  if (armed) {
    if (spellTargets(armed).has(name)) {
      staged = { kind: armed, at: name };
      playBurst(name, armed);
      armed = null;
      selected = null;
      hover = null;
      render();
    }
    return;
  }

  if (selected && destinationsFrom(selected).includes(name)) {
    playTurn(selected, name).catch((err) => showError(`Could not play that turn: ${err.message}`));
    return;
  }

  // A piece with no destinations refuses selection, which is how a frozen piece
  // visibly announces itself.
  selected = destinationsFrom(name).length > 0 ? name : null;
  render();
}

const board = $("board");

board.addEventListener("click", (event) => {
  const sq = event.target.closest(".sq");
  if (sq) onSquareClick(sq.dataset.square);
});

// `pointerdown` as well as `pointerover`: on touch there is no hover, so the
// zone preview would never appear without it.
for (const type of ["pointerover", "pointerdown"]) {
  board.addEventListener(type, (event) => {
    const sq = event.target.closest(".sq");
    if (!sq || armed !== "freeze") return;
    if (hover === sq.dataset.square) return;
    hover = sq.dataset.square;
    renderBoard();
  });
}

board.addEventListener("pointerleave", () => {
  if (hover === null) return;
  hover = null;
  renderBoard();
});

board.addEventListener("focusin", (event) => {
  const sq = event.target.closest(".sq");
  if (!sq) return;
  focused = sq.dataset.square;
  if (armed === "freeze" && hover !== focused) {
    hover = focused;
    renderBoard();
  }
});

const ARROWS = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, 1], ArrowDown: [0, -1] };

board.addEventListener("keydown", (event) => {
  const step = ARROWS[event.key];
  if (!step) return;
  event.preventDefault();
  const file = FILES.indexOf(focused[0]) + step[0];
  const rank = Number(focused[1]) - 1 + step[1];
  if (file < 0 || file > 7 || rank < 0 || rank > 7) return;
  focused = FILES[file] + (rank + 1);
  squares.get(focused).focus();
});

for (const kind of ["freeze", "jump"]) {
  $(`cast-${kind}`).addEventListener("click", () => {
    if (!myTurn()) return;
    armed = armed === kind ? null : kind;   // arming is a toggle
    selected = null;
    hover = null;
    render();
  });
}

$("cancel-spell").addEventListener("click", () => {
  staged = null;
  armed = null;
  selected = null;
  hover = null;
  render();
});

$("new-game").addEventListener("click", async () => {
  if (thinking) return;
  try {
    state = await call("reset");
    moveLog.length = 0;
    analysis = [];
    selected = null; staged = null; armed = null; hover = null;
    clearError();
    await refresh();
  } catch (err) {
    showError(`Could not start a new game: ${err.message}`);
  }
});

// One undo steps back a single turn -- the engine's reply. A second steps back
// the user's own, which is what "take that back" usually means, so the button
// undoes twice when there is a full pair to remove.
$("undo").addEventListener("click", async () => {
  if (thinking) return;
  try {
    for (let i = 0; i < 2; i++) {
      if (!(await call("undo"))) break;
      moveLog.pop();
    }
    state = await call("state");
    analysis = [];
    selected = null; staged = null; armed = null; hover = null;
    clearError();
    await refresh();
  } catch (err) {
    showError(`Could not undo: ${err.message}`);
  }
});

// Analysing runs the same search on the user's own position without applying
// the result, so the panel works when it is your move.
$("analyze").addEventListener("click", async () => {
  if (thinking) return;
  thinking = true;
  analysis = [];
  render();
  try {
    await call("search", { millis: Number($("millis").value) });
  } catch (err) {
    showError(`Analysis failed: ${err.message}`);
  } finally {
    thinking = false;
    render();
  }
});

onProgress = (msg) => {
  analysis.push({
    depth: msg.depth,
    eval: evalText(msg.score, state.sideToMove),
    text: msg.turn.text,
  });
  renderAnalysis();
};

// ── turn flow ────────────────────────────────────────────────────────────────

/// Promotion is auto-queened. Underpromotion is legal and the engine generates
/// it, but a picker is not worth the UI surface here; a human who wants a knight
/// can say so in an issue.
function promoFor(from, to) {
  const match = turnsForStaged().find((t) => t.from === from && t.to === to && t.promo !== null);
  return match ? "q" : null;
}

async function refresh() {
  legalTurns = await call("turns");
  render();
}

async function playTurn(from, to) {
  if (thinking) return;
  thinking = true;
  render(); // disables input and shows "Engine thinking…" before the first await
  const promo = promoFor(from, to);
  selected = null;
  const castThisTurn = staged;
  staged = null;
  try {
    const before = state;
    state = await call("apply", { from, to, promo, spell: castThisTurn });
    moveLog.push(describeTurn(before, { from, to, promo, spell: castThisTurn }));
    // render(), not refresh(): legalTurns is unused while thinking, and the
    // engine's round trip is about to start. finally re-fetches it below.
    render();
    if (state.status.kind === "inProgress") await engineMove();
  } catch (err) {
    showError(`Could not play that turn: ${err.message}`);
  } finally {
    thinking = false;
    await refresh();
  }
}

// No `finally` here: the caller (`playTurn`) owns clearing `thinking` and
// refreshing, since it must do that even when it never calls `engineMove` at
// all (e.g. the human's own turn ended the game).
async function engineMove() {
  thinking = true;
  analysis = [];
  render();
  try {
    const turn = await call("search", { millis: Number($("millis").value) });
    if (turn) {
      const before = state;
      state = await call("apply", {
        from: turn.from,
        to: turn.to,
        promo: turn.promo,
        spell: turn.spell,
      });
      moveLog.push(describeTurn(before, turn));
    }
  } catch (err) {
    showError(`Engine error: ${err.message}`);
  }
}

async function boot() {
  buildBoard();
  startPrimer();
  try {
    state = await call("state");
    await refresh();
  } catch (err) {
    showError(`Engine failed to start: ${err.message}`);
  }
}

boot();

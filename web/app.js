// The view. Holds no authoritative game state -- `state` below is only the last
// snapshot the worker sent, and every mutation goes back through the worker.
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

function showError(text) {
  const el = document.getElementById("error");
  el.textContent = text;
  el.hidden = false;
}

const GLYPHS = {
  wK: "♔", wQ: "♕", wR: "♖", wB: "♗", wN: "♘", wP: "♙",
  bK: "♚", bQ: "♛", bR: "♜", bB: "♝", bN: "♞", bP: "♟",
};

const FILES = "abcdefgh";
function squareName(index) {
  return FILES[index % 8] + (Math.floor(index / 8) + 1);
}

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
let staged = null;        // {kind, at} spell staged for this turn, or null
let thinking = false;     // blocks input while the engine searches
let armed = null;         // spell kind whose targets are being shown, or null

function sameSpell(a, b) {
  if (a === null && b === null) return true;
  if (a === null || b === null) return false;
  return a.kind === b.kind && a.at === b.at;
}

/// Turns available given whatever spell is currently staged.
function turnsForStaged() {
  return legalTurns.filter((t) => sameSpell(t.spell, staged));
}

function destinationsFrom(square) {
  return turnsForStaged().filter((t) => t.from === square).map((t) => t.to);
}

/// Squares this spell may legally target, taken from the legal turn list rather
/// than re-derived from the rules -- the engine already knows.
function spellTargets(kind) {
  const seen = new Set();
  for (const t of legalTurns) {
    if (t.spell && t.spell.kind === kind) seen.add(t.spell.at);
  }
  return seen;
}

function renderBoard() {
  const board = document.getElementById("board");
  board.replaceChildren();
  const destinations = new Set(selected ? destinationsFrom(selected) : []);
  const armedTargets = armed ? spellTargets(armed) : new Set();
  const frozen = new Set();
  const jumped = new Set();
  for (const f of state.fields) {
    if (f.kind === "jump") jumped.add(f.square);
    else for (const sq of freezeZone(f.square)) frozen.add(sq);
  }
  // Rank 8 first so the DOM reads top-to-bottom the way the board looks.
  for (let rank = 7; rank >= 0; rank--) {
    for (let file = 0; file < 8; file++) {
      const index = rank * 8 + file;
      const name = squareName(index);
      const sq = document.createElement("div");
      sq.className = `sq ${(rank + file) % 2 === 0 ? "dark" : "light"}`;
      sq.dataset.square = name;
      if (frozen.has(name)) sq.classList.add("frozen");
      if (jumped.has(name)) sq.classList.add("jumped");
      const code = state.board[index];
      if (code) {
        const piece = document.createElement("span");
        piece.className = `piece ${code[0] === "w" ? "white" : "black"}`;
        piece.textContent = GLYPHS[code];
        sq.append(piece);
      }
      if (selected === name) sq.classList.add("selected");
      if (selected && destinations.has(name)) sq.classList.add("target");
      if (armed && armedTargets.has(name)) sq.classList.add("spell-target");
      if (staged && staged.at === name) sq.classList.add("spell-target");
      board.append(sq);
    }
  }
}

function renderStatus() {
  const el = document.getElementById("status");
  const s = state.status;
  if (s.kind === "checkmate") el.textContent = `Checkmate — ${s.winner} wins`;
  else if (s.kind === "kingCaptured") el.textContent = `King captured — ${s.winner} wins`;
  else if (s.kind === "stalemate") el.textContent = "Stalemate — draw";
  else el.textContent = `${state.sideToMove === "white" ? "White" : "Black"} to move`;
}

function renderSpells() {
  const el = document.getElementById("spells");
  el.replaceChildren();
  for (const color of ["white", "black"]) {
    for (const kind of ["freeze", "jump"]) {
      const c = state.spells[color][kind];
      const row = document.createElement("div");
      row.className = "spell-row";
      const label = document.createElement("span");
      const cooldown = c.lock > 0 ? `cooldown ${c.lock}` : "ready";
      label.textContent = `${color} ${kind}: ${c.count} left, ${cooldown}`;
      if (color === state.sideToMove && color === "white") {
        const btn = document.createElement("button");
        btn.type = "button";
        btn.textContent = armed === kind ? "Pick target…" : `Cast ${kind}`;
        btn.disabled = thinking || staged !== null || spellTargets(kind).size === 0;
        btn.addEventListener("click", () => {
          armed = armed === kind ? null : kind;
          selected = null;
          render();
        });
        row.append(btn);
      }
      row.append(label);
      el.append(row);
    }
  }
  document.getElementById("cancel-spell").hidden = staged === null;
}

function render() {
  renderBoard();
  renderStatus();
  renderSpells();
  renderMoves();
}

function onSquareClick(name) {
  if (thinking || state.status.kind !== "inProgress") return;

  // Arming a spell: the click picks its target, and destinations are recomputed
  // for that cast -- moves illegal a moment ago may now be legal.
  if (armed) {
    if (spellTargets(armed).has(name)) {
      staged = { kind: armed, at: name };
      armed = null;
      selected = null;
      render();
    }
    return;
  }

  if (selected && destinationsFrom(selected).includes(name)) {
    playTurn(selected, name);
    return;
  }

  selected = destinationsFrom(name).length > 0 ? name : null;
  render();
}

document.getElementById("board").addEventListener("click", (event) => {
  const sq = event.target.closest(".sq");
  if (sq) onSquareClick(sq.dataset.square);
});

const moveLog = [];

function renderMoves() {
  const el = document.getElementById("moves");
  el.replaceChildren();
  for (const text of moveLog) {
    const li = document.createElement("li");
    li.textContent = text;
    el.append(li);
  }
  el.scrollTop = el.scrollHeight;
}

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
  const promo = promoFor(from, to);
  const chosen = turnsForStaged().find(
    (t) => t.from === from && t.to === to && t.promo === promo,
  );
  selected = null;
  const castThisTurn = staged;
  staged = null;
  try {
    state = await call("apply", { from, to, promo, spell: castThisTurn });
    moveLog.push(chosen ? chosen.text : `${from}${to}`);
    await refresh();
    if (state.status.kind === "inProgress") await engineMove();
  } catch (err) {
    showError(`Could not play that turn: ${err.message}`);
    await refresh();
  }
}

async function engineMove() {
  thinking = true;
  render();
  try {
    const millis = Number(document.getElementById("millis").value);
    const turn = await call("search", { millis });
    if (turn) {
      state = await call("apply", {
        from: turn.from,
        to: turn.to,
        promo: turn.promo,
        spell: turn.spell,
      });
      moveLog.push(turn.text);
    }
  } catch (err) {
    showError(`Engine error: ${err.message}`);
  } finally {
    thinking = false;
    await refresh();
  }
}

document.getElementById("new-game").addEventListener("click", async () => {
  if (thinking) return;
  state = await call("reset");
  moveLog.length = 0;
  selected = null; staged = null; armed = null;
  await refresh();
});

// One undo steps back a single turn -- the engine's reply. A second steps back
// the user's own, which is what "take that back" usually means, so the button
// undoes twice when there is a full pair to remove.
document.getElementById("undo").addEventListener("click", async () => {
  if (thinking) return;
  for (let i = 0; i < 2; i++) {
    if (!(await call("undo"))) break;
    moveLog.pop();
  }
  state = await call("state");
  selected = null; staged = null; armed = null;
  await refresh();
});

document.getElementById("cancel-spell").addEventListener("click", () => {
  staged = null; armed = null; selected = null;
  render();
});

async function boot() {
  try {
    state = await call("state");
    await refresh();
  } catch (err) {
    showError(`Engine failed to start: ${err.message}`);
  }
}

boot();

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

let state = null;

function renderBoard() {
  const board = document.getElementById("board");
  board.replaceChildren();
  const frozen = new Set();
  const jumped = new Set();
  for (const f of state.fields) {
    if (f.kind === "jump") jumped.add(f.square);
    else frozen.add(f.square);
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

function render() {
  renderBoard();
  renderStatus();
}

async function boot() {
  try {
    state = await call("state");
    render();
  } catch (err) {
    showError(`Engine failed to start: ${err.message}`);
  }
}

boot();

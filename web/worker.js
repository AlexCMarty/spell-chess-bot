// Owns the game. The main thread holds no authoritative state, so every question
// about legality or position comes here. Search runs here too, which is the whole
// reason this is a worker: a search takes seconds and would freeze the page.
import init, { Game } from "./pkg/spellchess_wasm.js";

let game = null;

const ready = init().then(() => {
  game = new Game();
});

function handle(msg) {
  switch (msg.cmd) {
    case "state":
      return JSON.parse(game.state_json());
    case "turns":
      return JSON.parse(game.legal_turns_json());
    case "apply": {
      const spell = msg.spell ?? null;
      const json = game.apply_turn(
        msg.from,
        msg.to,
        msg.promo ?? undefined,
        spell ? spell.kind : undefined,
        spell ? spell.at : undefined,
      );
      return JSON.parse(json);
    }
    case "search": {
      const onProgress = (depth, score, turnJson) => {
        self.postMessage({ type: "progress", depth, score, turn: JSON.parse(turnJson) });
      };
      const result = game.search(msg.millis, onProgress);
      return JSON.parse(result);
    }
    case "undo":
      return game.undo();
    case "reset":
      game.reset();
      return JSON.parse(game.state_json());
    default:
      throw new Error(`unknown command: ${msg.cmd}`);
  }
}

self.onmessage = async (event) => {
  const msg = event.data;
  try {
    await ready;
    self.postMessage({ id: msg.id, ok: true, data: handle(msg) });
  } catch (err) {
    // A panic inside the engine poisons the instance, so report it rather than
    // going silent and leaving the board frozen with no explanation.
    self.postMessage({ id: msg.id, ok: false, error: String(err && err.message ? err.message : err) });
  }
};

// The three tactic diagrams above the board. Each is a 6×6 checkerboard (drawn
// entirely by the CSS background) with pieces and spell fields as absolutely
// positioned boxes on top.
//
// Every card runs the same four-beat loop: one integer of state advanced by a
// setTimeout chain, where each beat only sets `transform`/`opacity`/`filter`.
// The browser tweens between beats, so there are no keyframes to keep in sync
// with the captions -- and the motion survives a re-render.
//
// The three duration arrays differ on purpose: in lockstep the row reads as one
// blinking widget rather than three separate tactics.

/// A cell's offset in the 6×6 grid. Each box is 16.6667% wide, so one cell is
/// 100% of its own width -- `translate` rather than `left/top` because the same
/// property then animates the slide.
function at(file, rank) {
  return `translate(${file * 100}%, ${(6 - rank) * 100}%)`;
}

const WHITE = "dg-cell dg-piece white";
const BLACK = "dg-cell dg-piece black";

// Freeze: the rook takes a defended knight for free, because the defending
// bishop is inside a frozen 3×3 and cannot recapture.
const FREEZE = {
  durations: [2100, 1500, 1600, 2600],
  captions: [
    "The rook eyes the knight — but the bishop guards it.",
    "Freeze the bishop's corner…",
    "…then take the knight, same turn.",
    "Nothing can recapture. A knight for free.",
  ],
  parts: [
    {
      // The 3×3 centred on e5: files d–f, ranks 4–6, i.e. the top-right quarter.
      cls: "dg-cell dg-field freeze",
      steps: [
        { opacity: 0, transform: "scale(.86)" },
        { opacity: 1, transform: "scale(1)" },
        { opacity: 1, transform: "scale(1)" },
        { opacity: 0, transform: "scale(.86)" },
      ],
    },
    {
      cls: BLACK, glyph: "♞",                      // c3, the target
      steps: [
        { opacity: 1, transform: at(2, 3) },
        { opacity: 1, transform: at(2, 3) },
        { opacity: 0, transform: at(2, 3) },
        { opacity: 0, transform: at(2, 3) },
      ],
    },
    {
      cls: BLACK, glyph: "♝",                      // e5, the frozen defender
      steps: [
        { transform: at(4, 5), filter: "none" },
        { transform: at(4, 5), filter: "drop-shadow(0 0 7px rgba(111,157,196,.95)) saturate(.7)" },
        { transform: at(4, 5), filter: "drop-shadow(0 0 7px rgba(111,157,196,.95)) saturate(.7)" },
        { transform: at(4, 5), filter: "none" },
      ],
    },
    {
      cls: WHITE, glyph: "♖",                      // c6 → c3
      steps: [
        { transform: at(2, 6) },
        { transform: at(2, 6) },
        { transform: at(2, 3) },
        { transform: at(2, 3) },
      ],
    },
  ],
};

// Jump: the bishop takes a queen straight through the caster's own pawn.
const JUMP = {
  durations: [2300, 1500, 1700, 2500],
  captions: [
    "Your own pawn blocks the bishop's diagonal.",
    "Jump makes that square see-through…",
    "…so the bishop slides straight past it.",
    "The queen falls — taken through your own pawn.",
  ],
  parts: [
    {
      cls: "dg-cell dg-field jump",                // c3
      steps: [
        { opacity: 0, transform: `${at(2, 3)} scale(.86)` },
        { opacity: 1, transform: `${at(2, 3)} scale(1)` },
        { opacity: 1, transform: `${at(2, 3)} scale(1)` },
        { opacity: 0, transform: `${at(2, 3)} scale(.86)` },
      ],
    },
    {
      cls: BLACK, glyph: "♛",                      // e5, the target
      steps: [
        { opacity: 1, transform: at(4, 5) },
        { opacity: 1, transform: at(4, 5) },
        { opacity: 0, transform: at(4, 5) },
        { opacity: 0, transform: at(4, 5) },
      ],
    },
    {
      cls: WHITE, glyph: "♙",                      // c3, ghosted while transparent
      steps: [
        { opacity: 1, transform: at(2, 3) },
        { opacity: .34, transform: at(2, 3) },
        { opacity: .34, transform: at(2, 3) },
        { opacity: 1, transform: at(2, 3) },
      ],
    },
    {
      cls: `${WHITE} long`, glyph: "♗",            // a1 → e5, over the pawn
      steps: [
        { transform: at(0, 1) },
        { transform: at(0, 1) },
        { transform: at(4, 5) },
        { transform: at(4, 5) },
      ],
    },
  ],
};

// King capture: the shield is jumped and the king is taken outright, in one
// turn, with no opportunity to answer. The whole reason this card exists.
const KING = {
  durations: [2500, 1400, 1700, 2800],
  captions: [
    "The king looks safe — the knight shields it.",
    "Jump the shield, in the same turn…",
    "…and the queen takes the king outright.",
    "No check, no answer. The game ends there.",
  ],
  parts: [
    {
      cls: "dg-cell dg-field jump",                // d4
      steps: [
        { opacity: 0, transform: `${at(3, 4)} scale(.86)` },
        { opacity: 1, transform: `${at(3, 4)} scale(1)` },
        { opacity: 1, transform: `${at(3, 4)} scale(1)` },
        { opacity: 0, transform: `${at(3, 4)} scale(.86)` },
      ],
    },
    {
      cls: "dg-cell dg-flash",                     // f4, fires on the capture only
      steps: [
        { opacity: 0, transform: `${at(5, 4)} scale(.6)` },
        { opacity: 0, transform: `${at(5, 4)} scale(.6)` },
        { opacity: 1, transform: `${at(5, 4)} scale(1.35)` },
        { opacity: 0, transform: `${at(5, 4)} scale(.6)` },
      ],
    },
    {
      cls: BLACK, glyph: "♞",                      // d4, the shield
      steps: [
        { opacity: 1, transform: at(3, 4) },
        { opacity: .34, transform: at(3, 4) },
        { opacity: .34, transform: at(3, 4) },
        { opacity: 1, transform: at(3, 4) },
      ],
    },
    {
      cls: BLACK, glyph: "♚",                      // f4
      steps: [
        { opacity: 1, transform: `${at(5, 4)} scale(1)` },
        { opacity: 1, transform: `${at(5, 4)} scale(1)` },
        { opacity: 0, transform: `${at(5, 4)} scale(1.5)` },
        { opacity: 0, transform: `${at(5, 4)} scale(1.5)` },
      ],
    },
    {
      cls: `${WHITE} long`, glyph: "♕",            // b4 → f4
      steps: [
        { transform: at(1, 4) },
        { transform: at(1, 4) },
        { transform: at(5, 4) },
        { transform: at(5, 4) },
      ],
    },
  ],
};

const CARDS = { freeze: FREEZE, jump: JUMP, king: KING };

function build(card, spec) {
  const board = card.querySelector("[data-diagram]");
  const caption = card.querySelector("[data-caption]");
  const nodes = spec.parts.map((part) => {
    const el = document.createElement("div");
    el.className = part.cls;
    if (part.glyph) el.textContent = part.glyph;
    board.append(el);
    return el;
  });

  const paint = (step) => {
    spec.parts.forEach((part, i) => Object.assign(nodes[i].style, part.steps[step]));
    caption.textContent = spec.captions[step];
  };
  return paint;
}

export function startPrimer() {
  const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  for (const card of document.querySelectorAll("[data-card]")) {
    const spec = CARDS[card.dataset.card];
    if (!spec) continue;
    const paint = build(card, spec);
    paint(0);
    if (still) continue;
    let step = 0;
    const tick = () => {
      step = (step + 1) % 4;
      paint(step);
      setTimeout(tick, spec.durations[step]);
    };
    setTimeout(tick, spec.durations[0]);
  }
}

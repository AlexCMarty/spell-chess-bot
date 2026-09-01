//! Per-call cost of the search hot path. Run before and after any change to
//! spell-turn generation:
//!   cargo run --release -p spellchess-search --example hotcost
//! Debug builds are meaningless here -- always use --release.

use std::hint::black_box;
use std::time::Instant;

use spellchess_core::{
    apply_turn, generate_quiescence_recapture_turns, generate_quiescence_turns_from,
    generate_search_turns, is_square_attacked, legal_moves, pseudo_legal_moves, Position, Turn,
};
use spellchess_search::eval::evaluate;
use spellchess_search::zobrist::hash_position;

fn bench<T>(label: &str, iters: u32, mut f: impl FnMut() -> T) {
    for _ in 0..(iters / 10).max(1) {
        black_box(f());
    }
    let start = Instant::now();
    for _ in 0..iters {
        black_box(f());
    }
    let ns = start.elapsed().as_nanos() as f64 / iters as f64;
    println!("{label:<42} {ns:>10.1} ns/call");
}

fn probe(name: &str, pos: &Position) {
    println!("\n=== {name} ===");
    let baseline = legal_moves(pos);
    let king = pos.board.king_square(pos.side_to_move).unwrap();
    let enemy = pos.side_to_move.opposite();
    println!("(legal moves: {})", baseline.len());

    bench("hash_position", 200_000, || hash_position(pos));
    bench("evaluate", 200_000, || evaluate(pos));
    bench("is_square_attacked(own king)", 200_000, || is_square_attacked(pos, king, enemy));
    bench("pseudo_legal_moves", 50_000, || pseudo_legal_moves(pos));
    bench("legal_moves", 50_000, || legal_moves(pos));
    bench("generate_quiescence_recapture_turns", 20_000, || {
        generate_quiescence_recapture_turns(pos, &baseline)
    });
    bench("generate_quiescence_turns_from", 2_000, || {
        generate_quiescence_turns_from(pos, &baseline)
    });
    bench("generate_search_turns", 500, || generate_search_turns(pos));
    let turn = Turn { spell: None, mv: baseline[0] };
    bench("apply_turn (no spell)", 200_000, || apply_turn(pos, &turn));
}

fn main() {
    let start = Position::starting();
    probe("starting position", &start);

    let mut mid = start;
    for _ in 0..6 {
        let moves = legal_moves(&mid);
        let turn = Turn { spell: None, mv: moves[moves.len() / 2] };
        mid = apply_turn(&mid, &turn);
    }
    probe("6 plies in", &mid);
}

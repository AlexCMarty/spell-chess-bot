use spellchess_core::{apply_turn, game_status, GameStatus, Position};
use crate::notation::parse_turn;
use crate::render::render_board;
use spellchess_search::search::{search as run_search, Budget};

const DEFAULT_GO_TIME: std::time::Duration = std::time::Duration::from_secs(5);
const GO_USAGE: &str = "usage: go | go --depth N | go --time SECONDS";

/// Parses `go`'s arguments. Malformed input is an error rather than a silent
/// fallback to the default budget: quietly running a different search than the
/// one asked for is worse than saying so.
fn parse_budget(rest: &str) -> Result<Budget, String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    match parts.as_slice() {
        [] => Ok(Budget::Time(DEFAULT_GO_TIME)),
        ["--depth", n] => n
            .parse()
            .map(Budget::Depth)
            .map_err(|_| format!("bad --depth value {n:?}: expected a whole number of plies. {GO_USAGE}")),
        ["--time", n] => n
            .parse()
            .map(|s| Budget::Time(std::time::Duration::from_secs(s)))
            .map_err(|_| format!("bad --time value {n:?}: expected a whole number of seconds. {GO_USAGE}")),
        _ => Err(format!("unrecognized go arguments {rest:?}. {GO_USAGE}")),
    }
}

pub struct Session {
    pub pos: Position,
    pub history: Vec<Position>,
}

impl Session {
    pub fn new() -> Self {
        Session { pos: Position::starting(), history: Vec::new() }
    }

    pub fn status(&self) -> GameStatus {
        game_status(&self.pos)
    }

    pub fn handle_command(&mut self, line: &str) -> String {
        let line = line.trim();
        match line {
            "" => String::new(),
            "board" => render_board(&self.pos),
            "fen" => format!("{:?}", self.pos),
            "undo" => match self.history.pop() {
                Some(prev) => {
                    self.pos = prev;
                    "undone".to_string()
                }
                None => "nothing to undo".to_string(),
            },
            "newgame" | "newgame white" | "newgame black" => {
                self.pos = Position::starting();
                self.history.clear();
                format!("new game, you are {}", if line.ends_with("black") { "black" } else { "white" })
            }
            line if line == "go" || line.starts_with("go ") => self.handle_go(line),
            other => self.apply_turn_command(other),
        }
    }

    fn apply_turn_command(&mut self, input: &str) -> String {
        match parse_turn(input, &self.pos) {
            Ok(turn) => {
                self.history.push(self.pos.clone());
                self.pos = apply_turn(&self.pos, &turn);
                match self.status() {
                    GameStatus::InProgress => "ok".to_string(),
                    GameStatus::Checkmate(w) => format!("checkmate, {w:?} wins"),
                    GameStatus::Stalemate => "stalemate -- draw".to_string(),
                    GameStatus::KingCaptured(w) => format!("king captured, {w:?} wins"),
                }
            }
            Err(e) => format!("error: {e}"),
        }
    }

    fn handle_go(&self, line: &str) -> String {
        let rest = line.strip_prefix("go").unwrap_or("").trim();
        let budget = match parse_budget(rest) {
            Ok(b) => b,
            Err(e) => return format!("error: {e}"),
        };
        match run_search(&self.pos, budget) {
            Some((turn, score)) => format!("suggest: {} (eval {})", crate::notation::format_turn(&turn), score),
            None => "no legal turn available".to_string(),
        }
    }
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spellchess_core::{Board, Color, Piece, PieceKind, Square};

    #[test]
    fn applying_a_move_updates_the_position_and_reports_ok() {
        let mut session = Session::new();
        assert_eq!(session.handle_command("e2e4"), "ok");
        assert_eq!(session.pos.board.get(Square::from_str("e4").unwrap()).unwrap().kind, PieceKind::Pawn);
    }

    #[test]
    fn undo_restores_the_previous_position() {
        let mut session = Session::new();
        session.handle_command("e2e4");
        assert_eq!(session.handle_command("undo"), "undone");
        assert!(session.pos.board.get(Square::from_str("e4").unwrap()).is_none());
    }

    #[test]
    fn illegal_move_is_reported_and_does_not_change_state() {
        let mut session = Session::new();
        let before = session.pos.clone();
        assert!(session.handle_command("e2e5").starts_with("error:"));
        assert_eq!(session.pos, before);
    }

    #[test]
    fn status_reports_checkmate_for_a_known_mate_position() {
        let mut session = Session::new();
        session.pos = Position { board: Board::empty(), ..Position::starting() };
        session.pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        session.pos.board.set(Square::from_str("e2").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        session.pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        session.pos.black_spells = spellchess_core::SpellState {
            freeze: spellchess_core::SpellCounter { count: 0, lock: 0 },
            jump: spellchess_core::SpellCounter { count: 0, lock: 0 },
        };
        session.pos.fields.push(spellchess_core::SpellField {
            square: Square::from_str("e8").unwrap(), owner: Color::White,
            kind: spellchess_core::SpellKind::Freeze, expires_after_ply: session.pos.ply + 1,
        });
        session.pos.side_to_move = Color::Black;
        assert_eq!(session.status(), GameStatus::Checkmate(Color::White));
    }

    #[test]
    fn bare_go_uses_the_five_second_default() {
        assert!(matches!(parse_budget(""), Ok(Budget::Time(d)) if d == std::time::Duration::from_secs(5)));
    }

    #[test]
    fn malformed_go_arguments_are_reported_not_silently_defaulted() {
        for bad in ["--depth abc", "--depth", "--nonsense", "--time later", "--depth 2 --time 3"] {
            assert!(parse_budget(bad).is_err(), "expected `go {bad}` to be rejected");
        }
        let mut session = Session::new();
        let out = session.handle_command("go --depth abc");
        assert!(out.starts_with("error:"), "expected an error message, got {out:?}");
    }

    #[test]
    fn go_returns_a_legal_suggestion() {
        let mut session = Session::new();
        // Use a minimal position instead of the full board: Position::starting() at depth >= 1 is
        // combinatorially intractable in debug builds due to unfiltered freeze/jump target
        // enumeration (Tasks 9/11). This test only needs to prove the `handle_go` wiring works
        // correctly and returns a legal move; it doesn't need to stress-test a full board.
        session.pos = Position { board: Board::empty(), ..Position::starting() };
        session.pos.board.set(Square::from_str("e1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::King }));
        session.pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        session.pos.board.set(Square::from_str("e8").unwrap(), Some(Piece { color: Color::Black, kind: PieceKind::King }));
        session.pos.white_spells = spellchess_core::SpellState {
            freeze: spellchess_core::SpellCounter { count: 0, lock: 0 },
            jump: spellchess_core::SpellCounter { count: 0, lock: 0 },
        };
        session.pos.black_spells = session.pos.white_spells;
        let output = session.handle_go("go --depth 2");
        assert!(output.starts_with("suggest: "));
        let mv_str = output.strip_prefix("suggest: ").unwrap().split(" (eval").next().unwrap();
        assert!(crate::notation::parse_turn(mv_str, &session.pos).is_ok());
    }
}

use spellchess_core::{apply_turn, game_status, GameStatus, Position};
use crate::notation::parse_turn;
use crate::render::render_board;

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
        session.pos.board.set(Square::from_str("a1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
        session.pos.board.set(Square::from_str("h1").unwrap(), Some(Piece { color: Color::White, kind: PieceKind::Rook }));
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
        assert_eq!(session.status(), GameStatus::Stalemate);
    }
}

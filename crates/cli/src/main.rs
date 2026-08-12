use spellchess_cli::repl::Session;
use std::io::{BufRead, Write};

fn main() {
    let mut session = Session::new();
    println!("Spell Chess advisor. Type 'quit' to exit.");
    let stdin = std::io::stdin();
    loop {
        print!("> ");
        std::io::stdout().flush().ok();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        if line.trim() == "quit" {
            break;
        }
        println!("{}", session.handle_command(&line));
    }
}

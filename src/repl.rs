use std::collections::HashMap;
use std::io::{self, Write};

use foxy::ast::Stmt;
use foxy::grid::render_for_repl;
use foxy::interpreter::Interpreter;
use foxy::lexer::{Lexer, Token};
use foxy::parser::Parser;
use foxy::value::Value;

const PROMPT: &str = "foxy> ";
const CONTINUATION_PROMPT: &str = "  ...> ";

pub fn run() {
    println!("Foxy REPL v0 — type :help for commands, :quit to exit.");

    let mut interpreter = Interpreter::new();
    let mut scopes: Vec<HashMap<String, Value>> = vec![HashMap::new()];
    let stdin = io::stdin();

    'entries: loop {
        let mut buffer = String::new();
        let mut prompt = PROMPT;

        loop {
            print!("{prompt}");
            let _ = io::stdout().flush();

            let mut line = String::new();
            if stdin.read_line(&mut line).unwrap_or(0) == 0 {
                println!();
                return; // EOF: Ctrl+D / Ctrl+Z
            }
            buffer.push_str(&line);

            if prompt == PROMPT {
                match buffer.trim() {
                    ":quit" | ":exit" => return,
                    ":help" => {
                        print_help();
                        continue 'entries;
                    }
                    "" => continue 'entries,
                    _ => {}
                }
            }

            if brace_depth(&buffer) <= 0 {
                break;
            }
            prompt = CONTINUATION_PROMPT;
        }

        run_entry(&buffer, &mut interpreter, &mut scopes);
    }
}

/// Counts unclosed `{` in `src` so the REPL knows whether a block (fn/if/
/// while/for body) is still open and more lines should be read before
/// attempting to run anything. Reuses the real lexer so brace characters
/// inside string literals or comments are never miscounted.
fn brace_depth(src: &str) -> i32 {
    match Lexer::new(src).tokenize() {
        Ok(tokens) => tokens.iter().fold(0, |depth, tok| match tok {
            Token::LBrace => depth + 1,
            Token::RBrace => depth - 1,
            _ => depth,
        }),
        Err(_) => 0, // let the real parse attempt below surface the actual error
    }
}

fn run_entry(src: &str, interpreter: &mut Interpreter, scopes: &mut Vec<HashMap<String, Value>>) {
    let tokens = match Lexer::new(src).tokenize() {
        Ok(tokens) => tokens,
        Err(e) => {
            eprintln!("lex error: {e}");
            return;
        }
    };

    let program = match Parser::new(tokens).parse_program() {
        Ok(program) => program,
        Err(e) => {
            eprintln!("parse error: {e}");
            return;
        }
    };

    // A single bare expression prints its value (unless it's null) instead
    // of silently discarding it -- e.g. typing `1 + 2` or `some_var`.
    if let [Stmt::ExprStmt(expr)] = program.as_slice() {
        match interpreter.eval_repl_expr(expr, scopes) {
            Ok(Value::Null) => {}
            Ok(v) => println!("{}", render_for_repl(&v)),
            Err(e) => eprintln!("runtime error: {e}"),
        }
        return;
    }

    if let Err(e) = interpreter.run_in_scopes(&program, scopes) {
        eprintln!("runtime error: {e}");
    }
}

fn print_help() {
    println!(
        "Type any statement or expression and press Enter.\n\
         Blocks ({{ ... }}) can span multiple lines -- the prompt changes\n\
         to '{CONTINUATION_PROMPT}' until they're closed.\n\
         A bare expression (e.g. `1 + 2` or `some_var`) prints its value;\n\
         query results (lists of records) print as a grid.\n\
         Variables and functions persist for the rest of the session.\n\
         \n\
         :help          show this message\n\
         :quit, :exit   leave the REPL"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_statement_has_zero_depth() {
        assert_eq!(brace_depth("var x = 5"), 0);
    }

    #[test]
    fn open_block_has_positive_depth() {
        assert_eq!(brace_depth("fn f() {"), 1);
    }

    #[test]
    fn balanced_block_has_zero_depth() {
        assert_eq!(brace_depth("fn f() { return 1 }"), 0);
    }

    #[test]
    fn nested_open_blocks_accumulate_depth() {
        assert_eq!(brace_depth("if a {\n    while b {"), 2);
    }

    #[test]
    fn braces_inside_string_literals_are_not_counted() {
        assert_eq!(brace_depth(r#"var s = "{ not a brace }""#), 0);
    }
}

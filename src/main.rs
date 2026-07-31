use std::process::ExitCode;

use foxy::lexer::Lexer;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("fmt") => cmd_fmt(&args[1..]),
        Some("lex") => cmd_lex(&args[1..]),
        Some(other) => {
            eprintln!("foxy: unknown command '{other}'\n");
            print_usage();
            ExitCode::FAILURE
        }
        None => {
            print_usage();
            ExitCode::SUCCESS
        }
    }
}

/// Debug command for the kernel work in progress: tokenizes a file and prints
/// the token stream. Not a real user-facing command yet — will likely be
/// replaced once there's a parser/interpreter to hand off to.
fn cmd_lex(paths: &[String]) -> ExitCode {
    let Some(path) = paths.first() else {
        eprintln!("usage: foxy lex <path>");
        return ExitCode::FAILURE;
    };

    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("foxy: failed to read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };

    match Lexer::new(&source).tokenize() {
        Ok(tokens) => {
            for tok in tokens {
                println!("{tok:?}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("foxy: lex error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_fmt(paths: &[String]) -> ExitCode {
    if paths.is_empty() {
        println!("foxy fmt: not yet implemented (would format all .foxy files in the project)");
    } else {
        for path in paths {
            println!("foxy fmt: not yet implemented (would format {path})");
        }
    }
    ExitCode::SUCCESS
}

fn print_usage() {
    println!(
        "foxy — the Foxy language toolchain\n\
         \n\
         USAGE:\n\
         \x20   foxy <command> [args]\n\
         \n\
         COMMANDS:\n\
         \x20   fmt [path...]    Format Foxy source files (placeholder — not yet implemented)"
    );
}

mod ast;
mod lexer;
mod parser;

use lexer::Lexer;
use parser::Parser;

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "examples/hello.foxy".to_string());

    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {path}: {e}"));

    println!("--- source: {path} ---");
    println!("{source}");

    let tokens = Lexer::new(&source).tokenize().unwrap_or_else(|e| {
        eprintln!("lex error: {e}");
        std::process::exit(1);
    });

    let ast = Parser::new(tokens).parse_program().unwrap_or_else(|e| {
        eprintln!("parse error: {e}");
        std::process::exit(1);
    });

    println!("--- parsed AST ({} top-level statements) ---", ast.len());
    for stmt in &ast {
        println!("{stmt:#?}");
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String),

    // keywords
    Var,
    Fn,
    Return,
    Select,
    From,
    Where,
    If,
    Else,
    While,
    For,
    In,
    Import,
    True,
    False,
    Null,
    ResultType,
    Ok,
    Err,

    // symbols
    Equals,
    Arrow,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    Bang,
    Question,
    Comma,
    Colon,
    Dot,

    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,

    Newline,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LexError {
    pub message: String,
    pub line: usize,
    pub col: usize,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.message)
    }
}

pub struct Lexer {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
}

type LexResult<T> = Result<T, LexError>;

impl Lexer {
    pub fn new(source: &str) -> Self {
        Lexer {
            chars: source.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    pub fn tokenize(mut self) -> LexResult<Vec<Token>> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token()?;
            let done = tok == Token::Eof;
            tokens.push(tok);
            if done {
                break;
            }
        }
        Ok(tokens)
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek();
        if let Some(c) = c {
            self.pos += 1;
            if c == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
        }
        c
    }

    fn error(&self, message: impl Into<String>) -> LexError {
        LexError {
            message: message.into(),
            line: self.line,
            col: self.col,
        }
    }

    fn next_token(&mut self) -> LexResult<Token> {
        self.skip_insignificant()?;

        let start_line = self.line;
        let start_col = self.col;
        let c = match self.advance() {
            Some(c) => c,
            None => return Ok(Token::Eof),
        };

        match c {
            '\n' => Ok(Token::Newline),
            '=' => Ok(self.two_char('=', Token::EqEq, Token::Equals)),
            '!' => Ok(self.two_char('=', Token::NotEq, Token::Bang)),
            '<' => Ok(self.two_char('=', Token::LtEq, Token::Lt)),
            '>' => Ok(self.two_char('=', Token::GtEq, Token::Gt)),
            '-' => Ok(self.two_char('>', Token::Arrow, Token::Minus)),
            '+' => Ok(Token::Plus),
            '*' => Ok(Token::Star),
            '/' => Ok(Token::Slash),
            '%' => Ok(Token::Percent),
            ',' => Ok(Token::Comma),
            ':' => Ok(Token::Colon),
            '.' => Ok(Token::Dot),
            '?' => Ok(Token::Question),
            '(' => Ok(Token::LParen),
            ')' => Ok(Token::RParen),
            '{' => Ok(Token::LBrace),
            '}' => Ok(Token::RBrace),
            '[' => Ok(Token::LBracket),
            ']' => Ok(Token::RBracket),
            '"' => self.read_string(start_line, start_col),
            c if c.is_ascii_digit() => self.read_number(c),
            c if c.is_alphabetic() || c == '_' => Ok(self.read_ident_or_keyword(c)),
            other => Err(self.error(format!("unexpected character '{other}'"))),
        }
    }

    /// If the next char matches `expected`, consumes it and returns `then`; otherwise returns `otherwise`.
    fn two_char(&mut self, expected: char, then: Token, otherwise: Token) -> Token {
        if self.peek() == Some(expected) {
            self.advance();
            then
        } else {
            otherwise
        }
    }

    fn skip_insignificant(&mut self) -> LexResult<()> {
        loop {
            match self.peek() {
                Some(' ') | Some('\t') | Some('\r') => {
                    self.advance();
                }
                Some('/') if self.peek_next() == Some('/') => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.advance();
                    }
                }
                Some('/') if self.peek_next() == Some('*') => {
                    self.skip_block_comment()?;
                }
                _ => break,
            }
        }
        Ok(())
    }

    fn skip_block_comment(&mut self) -> LexResult<()> {
        let start_line = self.line;
        let start_col = self.col;
        self.advance(); // '/'
        self.advance(); // '*'
        let mut depth = 1;
        while depth > 0 {
            match (self.peek(), self.peek_next()) {
                (Some('/'), Some('*')) => {
                    self.advance();
                    self.advance();
                    depth += 1;
                }
                (Some('*'), Some('/')) => {
                    self.advance();
                    self.advance();
                    depth -= 1;
                }
                (Some(_), _) => {
                    self.advance();
                }
                (None, _) => {
                    return Err(LexError {
                        message: "unterminated block comment".to_string(),
                        line: start_line,
                        col: start_col,
                    });
                }
            }
        }
        Ok(())
    }

    fn read_string(&mut self, start_line: usize, start_col: usize) -> LexResult<Token> {
        let mut s = String::new();
        loop {
            match self.advance() {
                Some('"') => return Ok(Token::Str(s)),
                Some('\\') => match self.advance() {
                    Some('"') => s.push('"'),
                    Some('\\') => s.push('\\'),
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some(other) => {
                        return Err(self.error(format!("unknown escape sequence '\\{other}'")));
                    }
                    None => {
                        return Err(LexError {
                            message: "unterminated string literal".to_string(),
                            line: start_line,
                            col: start_col,
                        });
                    }
                },
                Some(c) => s.push(c),
                None => {
                    return Err(LexError {
                        message: "unterminated string literal".to_string(),
                        line: start_line,
                        col: start_col,
                    });
                }
            }
        }
    }

    fn read_number(&mut self, first: char) -> LexResult<Token> {
        let mut s = String::new();
        s.push(first);
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }

        // A '.' is only part of this number if followed by a digit — otherwise
        // it belongs to a later token (e.g. member access on an int literal).
        if self.peek() == Some('.') && self.peek_next().is_some_and(|c| c.is_ascii_digit()) {
            s.push('.');
            self.advance();
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    s.push(c);
                    self.advance();
                } else {
                    break;
                }
            }
            return s
                .parse::<f64>()
                .map(Token::Float)
                .map_err(|e| self.error(format!("invalid float literal '{s}': {e}")));
        }

        s.parse::<i64>()
            .map(Token::Int)
            .map_err(|e| self.error(format!("invalid integer literal '{s}': {e}")))
    }

    fn read_ident_or_keyword(&mut self, first: char) -> Token {
        let mut s = String::new();
        s.push(first);
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }
        match s.as_str() {
            "var" => Token::Var,
            "fn" => Token::Fn,
            "return" => Token::Return,
            "select" => Token::Select,
            "from" => Token::From,
            "where" => Token::Where,
            "if" => Token::If,
            "else" => Token::Else,
            "while" => Token::While,
            "for" => Token::For,
            "in" => Token::In,
            "import" => Token::Import,
            "true" => Token::True,
            "false" => Token::False,
            "null" => Token::Null,
            "Result" => Token::ResultType,
            "Ok" => Token::Ok,
            "Err" => Token::Err,
            _ => Token::Ident(s),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(src: &str) -> Vec<Token> {
        Lexer::new(src).tokenize().expect("lex should succeed")
    }

    fn lex_err(src: &str) -> LexError {
        Lexer::new(src).tokenize().expect_err("lex should fail")
    }

    #[test]
    fn lexes_var_decl_with_int() {
        assert_eq!(
            lex("var age = 30"),
            vec![
                Token::Var,
                Token::Ident("age".into()),
                Token::Equals,
                Token::Int(30),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn lexes_float_literal() {
        assert_eq!(
            lex("var pi = 3.5"),
            vec![
                Token::Var,
                Token::Ident("pi".into()),
                Token::Equals,
                Token::Float(3.5),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn dot_without_trailing_digit_is_not_part_of_a_float() {
        // e.g. `30.to_string()` — the '.' is member access, not a decimal point.
        assert_eq!(
            lex("30.foo"),
            vec![
                Token::Int(30),
                Token::Dot,
                Token::Ident("foo".into()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn lexes_string_with_escapes() {
        assert_eq!(
            lex(r#""line1\nline2\t\"quoted\"""#),
            vec![Token::Str("line1\nline2\t\"quoted\"".into()), Token::Eof]
        );
    }

    #[test]
    fn lexes_bool_and_null_literals() {
        assert_eq!(
            lex("true false null"),
            vec![Token::True, Token::False, Token::Null, Token::Eof]
        );
    }

    #[test]
    fn lexes_result_ok_err_keywords() {
        assert_eq!(
            lex("Result Ok Err"),
            vec![Token::ResultType, Token::Ok, Token::Err, Token::Eof]
        );
    }

    #[test]
    fn lexes_control_flow_keywords() {
        assert_eq!(
            lex("if else while for in import"),
            vec![
                Token::If,
                Token::Else,
                Token::While,
                Token::For,
                Token::In,
                Token::Import,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn lexes_list_and_record_punctuation() {
        assert_eq!(
            lex("[1, 2] { x: 1 }"),
            vec![
                Token::LBracket,
                Token::Int(1),
                Token::Comma,
                Token::Int(2),
                Token::RBracket,
                Token::LBrace,
                Token::Ident("x".into()),
                Token::Colon,
                Token::Int(1),
                Token::RBrace,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn lexes_percent_operator() {
        assert_eq!(
            lex("n % 3"),
            vec![
                Token::Ident("n".into()),
                Token::Percent,
                Token::Int(3),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn lexes_bang_vs_not_eq() {
        assert_eq!(
            lex("!ok != done"),
            vec![
                Token::Bang,
                Token::Ident("ok".into()),
                Token::NotEq,
                Token::Ident("done".into()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn lexes_question_mark_postfix() {
        assert_eq!(
            lex("foo()?"),
            vec![
                Token::Ident("foo".into()),
                Token::LParen,
                Token::RParen,
                Token::Question,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn skips_line_and_block_comments() {
        assert_eq!(
            lex("var x = 1 // trailing\n/* block\n   spanning lines */ var y = 2"),
            vec![
                Token::Var,
                Token::Ident("x".into()),
                Token::Equals,
                Token::Int(1),
                Token::Newline,
                Token::Var,
                Token::Ident("y".into()),
                Token::Equals,
                Token::Int(2),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn skips_nested_block_comments() {
        assert_eq!(
            lex("/* outer /* inner */ still outer */ var x = 1"),
            vec![
                Token::Var,
                Token::Ident("x".into()),
                Token::Equals,
                Token::Int(1),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn rejects_unterminated_string() {
        let err = lex_err(r#"var x = "oops"#);
        assert!(err.message.contains("unterminated string"));
    }

    #[test]
    fn rejects_unterminated_block_comment() {
        let err = lex_err("var x = 1 /* never closed");
        assert!(err.message.contains("unterminated block comment"));
    }

    #[test]
    fn rejects_unknown_escape_sequence() {
        let err = lex_err(r#""bad \q escape""#);
        assert!(err.message.contains("unknown escape sequence"));
    }

    #[test]
    fn tracks_line_and_column_in_errors() {
        let err = lex_err("var x = 1\nvar y = \"unterminated");
        assert_eq!(err.line, 2);
    }

    #[test]
    fn lexes_full_kitchen_sink_example() {
        let src = r#"import "std/strings"

var name = "Alice"
var age = 30
var tags = ["vip", "beta"]
var profile = { name: name, age: age, active: true }

fn greet(person: string) -> string {
    if person == "" {
        return "Hello, stranger"
    } else {
        return "Hello, " + strings.upper(person)
    }
}

fn classify(age: int) -> Result<string, string> {
    if age < 0 {
        return Err("negative age")
    }
    return Ok("adult")
}

var label = classify(age)?
var adults = select name, age from customers where age >= 18
"#;
        let tokens = Lexer::new(src).tokenize().expect("kitchen sink should lex");
        assert_eq!(tokens.last(), Some(&Token::Eof));
        assert!(tokens.contains(&Token::ResultType));
        assert!(tokens.contains(&Token::Question));
        assert!(tokens.contains(&Token::Select));
    }
}

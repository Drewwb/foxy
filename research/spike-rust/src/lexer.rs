#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    Int(i64),
    Str(String),

    Var,
    Fn,
    Return,
    Select,
    From,
    Where,

    Equals,
    Arrow,
    Plus,
    Minus,
    Star,
    Slash,
    EqEq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    Comma,
    Colon,

    LParen,
    RParen,
    LBrace,
    RBrace,

    Newline,
    Eof,
}

pub struct Lexer {
    chars: Vec<char>,
    pos: usize,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        Lexer {
            chars: source.chars().collect(),
            pos: 0,
        }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, String> {
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
        self.pos += 1;
        c
    }

    fn next_token(&mut self) -> Result<Token, String> {
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
                _ => break,
            }
        }

        let c = match self.advance() {
            Some(c) => c,
            None => return Ok(Token::Eof),
        };

        match c {
            '\n' => Ok(Token::Newline),
            '=' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Ok(Token::EqEq)
                } else {
                    Ok(Token::Equals)
                }
            }
            '!' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Ok(Token::NotEq)
                } else {
                    Err("unexpected character '!'".to_string())
                }
            }
            '<' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Ok(Token::LtEq)
                } else {
                    Ok(Token::Lt)
                }
            }
            '>' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Ok(Token::GtEq)
                } else {
                    Ok(Token::Gt)
                }
            }
            '-' => {
                if self.peek() == Some('>') {
                    self.advance();
                    Ok(Token::Arrow)
                } else {
                    Ok(Token::Minus)
                }
            }
            '+' => Ok(Token::Plus),
            '*' => Ok(Token::Star),
            '/' => Ok(Token::Slash),
            ',' => Ok(Token::Comma),
            ':' => Ok(Token::Colon),
            '(' => Ok(Token::LParen),
            ')' => Ok(Token::RParen),
            '{' => Ok(Token::LBrace),
            '}' => Ok(Token::RBrace),
            '"' => self.read_string(),
            c if c.is_ascii_digit() => self.read_number(c),
            c if c.is_alphabetic() || c == '_' => self.read_ident_or_keyword(c),
            other => Err(format!("unexpected character '{other}'")),
        }
    }

    fn read_string(&mut self) -> Result<Token, String> {
        let mut s = String::new();
        loop {
            match self.advance() {
                Some('"') => return Ok(Token::Str(s)),
                Some(c) => s.push(c),
                None => return Err("unterminated string literal".to_string()),
            }
        }
    }

    fn read_number(&mut self, first: char) -> Result<Token, String> {
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
        s.parse::<i64>()
            .map(Token::Int)
            .map_err(|e| format!("invalid integer literal '{s}': {e}"))
    }

    fn read_ident_or_keyword(&mut self, first: char) -> Result<Token, String> {
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
        Ok(match s.as_str() {
            "var" => Token::Var,
            "fn" => Token::Fn,
            "return" => Token::Return,
            "select" => Token::Select,
            "from" => Token::From,
            "where" => Token::Where,
            _ => Token::Ident(s),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(src: &str) -> Vec<Token> {
        Lexer::new(src).tokenize().expect("lex should succeed")
    }

    #[test]
    fn lexes_var_decl() {
        let tokens = lex("var age = 30");
        assert_eq!(
            tokens,
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
    fn lexes_string_literal() {
        let tokens = lex(r#"var name = "Alice""#);
        assert_eq!(
            tokens,
            vec![
                Token::Var,
                Token::Ident("name".into()),
                Token::Equals,
                Token::Str("Alice".into()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn lexes_comparison_operators() {
        let tokens = lex("age >= 18");
        assert_eq!(
            tokens,
            vec![
                Token::Ident("age".into()),
                Token::GtEq,
                Token::Int(18),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn lexes_arrow_and_keywords() {
        let tokens = lex("fn greet() -> string {}");
        assert_eq!(
            tokens,
            vec![
                Token::Fn,
                Token::Ident("greet".into()),
                Token::LParen,
                Token::RParen,
                Token::Arrow,
                Token::Ident("string".into()),
                Token::LBrace,
                Token::RBrace,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn skips_line_comments() {
        let tokens = lex("var x = 1 // trailing comment\nvar y = 2");
        assert_eq!(
            tokens,
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
    fn rejects_unterminated_string() {
        let err = Lexer::new(r#"var x = "oops"#).tokenize().unwrap_err();
        assert!(err.contains("unterminated"));
    }
}

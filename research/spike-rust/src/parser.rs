use crate::ast::{BinOp, Expr, Param, Stmt};
use crate::lexer::Token;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

type ParseResult<T> = Result<T, String>;

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0 }
    }

    pub fn parse_program(&mut self) -> ParseResult<Vec<Stmt>> {
        let mut stmts = Vec::new();
        self.skip_newlines();
        while !self.check(&Token::Eof) {
            stmts.push(self.parse_stmt()?);
            self.end_of_statement()?;
        }
        Ok(stmts)
    }

    // --- statements ---

    fn parse_stmt(&mut self) -> ParseResult<Stmt> {
        match self.peek() {
            Token::Var => self.parse_var_decl(),
            Token::Fn => self.parse_fn_decl(),
            Token::Return => self.parse_return(),
            _ => Ok(Stmt::ExprStmt(self.parse_expr()?)),
        }
    }

    fn parse_var_decl(&mut self) -> ParseResult<Stmt> {
        self.expect(Token::Var)?;
        let name = self.expect_ident()?;
        self.expect(Token::Equals)?;
        let value = self.parse_expr()?;
        Ok(Stmt::VarDecl { name, value })
    }

    fn parse_fn_decl(&mut self) -> ParseResult<Stmt> {
        self.expect(Token::Fn)?;
        let name = self.expect_ident()?;
        self.expect(Token::LParen)?;
        let mut params = Vec::new();
        if !self.check(&Token::RParen) {
            loop {
                let pname = self.expect_ident()?;
                self.expect(Token::Colon)?;
                let ptype = self.expect_ident()?;
                params.push(Param {
                    name: pname,
                    type_name: ptype,
                });
                if self.check(&Token::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.expect(Token::RParen)?;

        let return_type = if self.check(&Token::Arrow) {
            self.advance();
            Some(self.expect_ident()?)
        } else {
            None
        };

        self.expect(Token::LBrace)?;
        self.skip_newlines();
        let mut body = Vec::new();
        while !self.check(&Token::RBrace) {
            body.push(self.parse_stmt()?);
            self.end_of_statement()?;
        }
        self.expect(Token::RBrace)?;

        Ok(Stmt::FnDecl {
            name,
            params,
            return_type,
            body,
        })
    }

    fn parse_return(&mut self) -> ParseResult<Stmt> {
        self.expect(Token::Return)?;
        let value = self.parse_expr()?;
        Ok(Stmt::Return(value))
    }

    /// A statement must end at a newline, EOF, or a block-closing `}`.
    fn end_of_statement(&mut self) -> ParseResult<()> {
        match self.peek() {
            Token::Newline => {
                self.skip_newlines();
                Ok(())
            }
            Token::Eof | Token::RBrace => Ok(()),
            other => Err(format!("expected end of statement, found {other:?}")),
        }
    }

    fn skip_newlines(&mut self) {
        while self.check(&Token::Newline) {
            self.advance();
        }
    }

    // --- expressions (precedence climbing) ---

    fn parse_expr(&mut self) -> ParseResult<Expr> {
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> ParseResult<Expr> {
        let mut left = self.parse_additive()?;
        loop {
            let op = match self.peek() {
                Token::EqEq => BinOp::Eq,
                Token::NotEq => BinOp::NotEq,
                Token::Lt => BinOp::Lt,
                Token::LtEq => BinOp::LtEq,
                Token::Gt => BinOp::Gt,
                Token::GtEq => BinOp::GtEq,
                _ => break,
            };
            self.advance();
            let right = self.parse_additive()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> ParseResult<Expr> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> ParseResult<Expr> {
        let mut left = self.parse_primary()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                _ => break,
            };
            self.advance();
            let right = self.parse_primary()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_primary(&mut self) -> ParseResult<Expr> {
        match self.peek().clone() {
            Token::Int(n) => {
                self.advance();
                Ok(Expr::Int(n))
            }
            Token::Str(s) => {
                self.advance();
                Ok(Expr::Str(s))
            }
            Token::Ident(name) => {
                self.advance();
                Ok(Expr::Ident(name))
            }
            Token::LParen => {
                self.advance();
                let inner = self.parse_expr()?;
                self.expect(Token::RParen)?;
                Ok(inner)
            }
            Token::Select => self.parse_select(),
            other => Err(format!("unexpected token in expression: {other:?}")),
        }
    }

    fn parse_select(&mut self) -> ParseResult<Expr> {
        self.expect(Token::Select)?;
        let mut columns = vec![self.expect_ident()?];
        while self.check(&Token::Comma) {
            self.advance();
            columns.push(self.expect_ident()?);
        }
        self.expect(Token::From)?;
        let table = self.expect_ident()?;
        let where_clause = if self.check(&Token::Where) {
            self.advance();
            Some(Box::new(self.parse_expr()?))
        } else {
            None
        };
        Ok(Expr::Select {
            columns,
            table,
            where_clause,
        })
    }

    // --- token helpers ---

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens[self.pos].clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    fn check(&self, tok: &Token) -> bool {
        self.peek() == tok
    }

    fn expect(&mut self, tok: Token) -> ParseResult<()> {
        if self.check(&tok) {
            self.advance();
            Ok(())
        } else {
            Err(format!("expected {tok:?}, found {:?}", self.peek()))
        }
    }

    fn expect_ident(&mut self) -> ParseResult<String> {
        match self.peek().clone() {
            Token::Ident(name) => {
                self.advance();
                Ok(name)
            }
            other => Err(format!("expected identifier, found {other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse(src: &str) -> Vec<Stmt> {
        let tokens = Lexer::new(src).tokenize().expect("lex should succeed");
        Parser::new(tokens)
            .parse_program()
            .expect("parse should succeed")
    }

    #[test]
    fn parses_var_decl_with_int() {
        let stmts = parse("var age = 30");
        assert_eq!(
            stmts,
            vec![Stmt::VarDecl {
                name: "age".into(),
                value: Expr::Int(30),
            }]
        );
    }

    #[test]
    fn parses_var_decl_with_string() {
        let stmts = parse(r#"var name = "Alice""#);
        assert_eq!(
            stmts,
            vec![Stmt::VarDecl {
                name: "name".into(),
                value: Expr::Str("Alice".into()),
            }]
        );
    }

    #[test]
    fn parses_fn_decl_with_return() {
        let stmts = parse(
            r#"fn greet(person: string) -> string {
    return "Hello, " + person
}"#,
        );
        assert_eq!(
            stmts,
            vec![Stmt::FnDecl {
                name: "greet".into(),
                params: vec![Param {
                    name: "person".into(),
                    type_name: "string".into(),
                }],
                return_type: Some("string".into()),
                body: vec![Stmt::Return(Expr::Binary {
                    op: BinOp::Add,
                    left: Box::new(Expr::Str("Hello, ".into())),
                    right: Box::new(Expr::Ident("person".into())),
                })],
            }]
        );
    }

    #[test]
    fn parses_select_with_where() {
        let stmts = parse("var result = select name, age from customers where age >= 18");
        assert_eq!(
            stmts,
            vec![Stmt::VarDecl {
                name: "result".into(),
                value: Expr::Select {
                    columns: vec!["name".into(), "age".into()],
                    table: "customers".into(),
                    where_clause: Some(Box::new(Expr::Binary {
                        op: BinOp::GtEq,
                        left: Box::new(Expr::Ident("age".into())),
                        right: Box::new(Expr::Int(18)),
                    })),
                },
            }]
        );
    }

    #[test]
    fn parses_select_without_where() {
        let stmts = parse("var result = select name from customers");
        assert_eq!(
            stmts,
            vec![Stmt::VarDecl {
                name: "result".into(),
                value: Expr::Select {
                    columns: vec!["name".into()],
                    table: "customers".into(),
                    where_clause: None,
                },
            }]
        );
    }

    #[test]
    fn parses_full_example_program() {
        let stmts = parse(
            r#"var name = "Alice"
var age = 30

fn greet(person: string) -> string {
    return "Hello, " + person
}

var result = select name, age from customers where age >= 18
"#,
        );
        assert_eq!(stmts.len(), 4);
    }

    #[test]
    fn reports_error_on_missing_from() {
        let tokens = Lexer::new("var x = select name customers").tokenize().unwrap();
        let err = Parser::new(tokens).parse_program().unwrap_err();
        assert!(err.contains("From"));
    }
}

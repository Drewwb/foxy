use crate::ast::{BinOp, Expr, Param, Stmt, TypeExpr, UnaryOp};
use crate::lexer::Token;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

pub type ParseResult<T> = Result<T, String>;

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
            Token::If => self.parse_if_stmt(),
            Token::While => self.parse_while_stmt(),
            Token::For => self.parse_for_stmt(),
            Token::Return => self.parse_return(),
            Token::Import => self.parse_import(),
            Token::Open => self.parse_open(),
            Token::Ident(_) if self.peek_at(1) == &Token::Equals => self.parse_assign(),
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

    /// Reassignment of an existing binding: `IDENT "=" expr`. Distinct from
    /// `var_decl`, which introduces a new binding. No general lvalues yet
    /// (no `record.field = ...`) -- only bare identifiers.
    fn parse_assign(&mut self) -> ParseResult<Stmt> {
        let name = self.expect_ident()?;
        self.expect(Token::Equals)?;
        let value = self.parse_expr()?;
        Ok(Stmt::Assign { name, value })
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
                let type_expr = self.parse_type()?;
                params.push(Param {
                    name: pname,
                    type_expr,
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
            Some(self.parse_type()?)
        } else {
            None
        };

        let body = self.parse_block()?;

        Ok(Stmt::FnDecl {
            name,
            params,
            return_type,
            body,
        })
    }

    /// `IDENT ("<" type ("," type)* ">")?`
    fn parse_type(&mut self) -> ParseResult<TypeExpr> {
        let name = match self.peek().clone() {
            Token::Ident(name) => {
                self.advance();
                name
            }
            Token::ResultType => {
                self.advance();
                "Result".to_string()
            }
            other => return Err(format!("expected type name, found {other:?}")),
        };

        let mut args = Vec::new();
        if self.check(&Token::Lt) {
            self.advance();
            args.push(self.parse_type()?);
            while self.check(&Token::Comma) {
                self.advance();
                args.push(self.parse_type()?);
            }
            self.expect(Token::Gt)?;
        }

        Ok(TypeExpr { name, args })
    }

    fn parse_if_stmt(&mut self) -> ParseResult<Stmt> {
        self.expect(Token::If)?;
        let condition = self.parse_expr()?;
        let then_branch = self.parse_block()?;

        let else_branch = if self.check(&Token::Else) {
            self.advance();
            if self.check(&Token::If) {
                Some(vec![self.parse_if_stmt()?])
            } else {
                Some(self.parse_block()?)
            }
        } else {
            None
        };

        Ok(Stmt::If {
            condition,
            then_branch,
            else_branch,
        })
    }

    fn parse_while_stmt(&mut self) -> ParseResult<Stmt> {
        self.expect(Token::While)?;
        let condition = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::While { condition, body })
    }

    fn parse_for_stmt(&mut self) -> ParseResult<Stmt> {
        self.expect(Token::For)?;
        let var_name = self.expect_ident()?;
        self.expect(Token::In)?;
        let iterable = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::For {
            var_name,
            iterable,
            body,
        })
    }

    fn parse_return(&mut self) -> ParseResult<Stmt> {
        self.expect(Token::Return)?;
        let value = if self.starts_expr() {
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Stmt::Return(value))
    }

    fn parse_import(&mut self) -> ParseResult<Stmt> {
        self.expect(Token::Import)?;
        match self.peek().clone() {
            Token::Str(path) => {
                self.advance();
                Ok(Stmt::Import(path))
            }
            other => Err(format!(
                "expected a string path after 'import', found {other:?}"
            )),
        }
    }

    /// `open "path.db"`. The path must be a string literal for now.
    fn parse_open(&mut self) -> ParseResult<Stmt> {
        self.expect(Token::Open)?;
        match self.peek().clone() {
            Token::Str(path) => {
                self.advance();
                Ok(Stmt::Open(path))
            }
            other => Err(format!(
                "expected a string path after 'open', found {other:?}"
            )),
        }
    }

    fn starts_expr(&self) -> bool {
        !matches!(self.peek(), Token::Newline | Token::RBrace | Token::Eof)
    }

    fn parse_block(&mut self) -> ParseResult<Vec<Stmt>> {
        self.expect(Token::LBrace)?;
        self.skip_newlines();
        let mut stmts = Vec::new();
        while !self.check(&Token::RBrace) {
            stmts.push(self.parse_stmt()?);
            self.end_of_statement()?;
        }
        self.expect(Token::RBrace)?;
        Ok(stmts)
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
        self.parse_try_expr()
    }

    fn parse_try_expr(&mut self) -> ParseResult<Expr> {
        let expr = self.parse_comparison()?;
        if self.check(&Token::Question) {
            self.advance();
            Ok(Expr::Try(Box::new(expr)))
        } else {
            Ok(expr)
        }
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
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                Token::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> ParseResult<Expr> {
        let op = match self.peek() {
            Token::Minus => Some(UnaryOp::Neg),
            Token::Bang => Some(UnaryOp::Not),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let expr = self.parse_call()?;
            Ok(Expr::Unary {
                op,
                expr: Box::new(expr),
            })
        } else {
            self.parse_call()
        }
    }

    fn parse_call(&mut self) -> ParseResult<Expr> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek() {
                Token::LParen => {
                    self.advance();
                    let mut args = Vec::new();
                    if !self.check(&Token::RParen) {
                        args.push(self.parse_expr()?);
                        while self.check(&Token::Comma) {
                            self.advance();
                            args.push(self.parse_expr()?);
                        }
                    }
                    self.expect(Token::RParen)?;
                    expr = Expr::Call {
                        callee: Box::new(expr),
                        args,
                    };
                }
                Token::Dot => {
                    self.advance();
                    let field = self.expect_ident()?;
                    expr = Expr::FieldAccess {
                        target: Box::new(expr),
                        field,
                    };
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> ParseResult<Expr> {
        match self.peek().clone() {
            Token::Int(n) => {
                self.advance();
                Ok(Expr::Int(n))
            }
            Token::Float(f) => {
                self.advance();
                Ok(Expr::Float(f))
            }
            Token::Str(s) => {
                self.advance();
                Ok(Expr::Str(s))
            }
            Token::True => {
                self.advance();
                Ok(Expr::Bool(true))
            }
            Token::False => {
                self.advance();
                Ok(Expr::Bool(false))
            }
            Token::Null => {
                self.advance();
                Ok(Expr::Null)
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
            Token::LBracket => self.parse_list_lit(),
            Token::LBrace => self.parse_record_lit(),
            Token::Select => self.parse_select(),
            Token::Ok => {
                self.advance();
                self.expect(Token::LParen)?;
                let inner = self.parse_expr()?;
                self.expect(Token::RParen)?;
                Ok(Expr::Ok(Box::new(inner)))
            }
            Token::Err => {
                self.advance();
                self.expect(Token::LParen)?;
                let inner = self.parse_expr()?;
                self.expect(Token::RParen)?;
                Ok(Expr::Err(Box::new(inner)))
            }
            other => Err(format!("unexpected token in expression: {other:?}")),
        }
    }

    fn parse_list_lit(&mut self) -> ParseResult<Expr> {
        self.expect(Token::LBracket)?;
        let mut elements = Vec::new();
        if !self.check(&Token::RBracket) {
            elements.push(self.parse_expr()?);
            while self.check(&Token::Comma) {
                self.advance();
                elements.push(self.parse_expr()?);
            }
        }
        self.expect(Token::RBracket)?;
        Ok(Expr::List(elements))
    }

    fn parse_record_lit(&mut self) -> ParseResult<Expr> {
        self.expect(Token::LBrace)?;
        let mut fields = Vec::new();
        if !self.check(&Token::RBrace) {
            fields.push(self.parse_record_field()?);
            while self.check(&Token::Comma) {
                self.advance();
                fields.push(self.parse_record_field()?);
            }
        }
        self.expect(Token::RBrace)?;
        Ok(Expr::Record(fields))
    }

    fn parse_record_field(&mut self) -> ParseResult<(String, Expr)> {
        let name = self.expect_ident()?;
        self.expect(Token::Colon)?;
        let value = self.parse_expr()?;
        Ok((name, value))
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

    fn peek_at(&self, offset: usize) -> &Token {
        &self.tokens[(self.pos + offset).min(self.tokens.len() - 1)]
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

    fn parse_err(src: &str) -> String {
        let tokens = Lexer::new(src).tokenize().expect("lex should succeed");
        Parser::new(tokens)
            .parse_program()
            .expect_err("parse should fail")
    }

    fn simple_type(name: &str) -> TypeExpr {
        TypeExpr {
            name: name.to_string(),
            args: vec![],
        }
    }

    #[test]
    fn parses_var_decl_with_int() {
        assert_eq!(
            parse("var age = 30"),
            vec![Stmt::VarDecl {
                name: "age".into(),
                value: Expr::Int(30),
            }]
        );
    }

    #[test]
    fn parses_reassignment_distinct_from_var_decl() {
        assert_eq!(
            parse("var total = 0\ntotal = total + 1"),
            vec![
                Stmt::VarDecl {
                    name: "total".into(),
                    value: Expr::Int(0),
                },
                Stmt::Assign {
                    name: "total".into(),
                    value: Expr::Binary {
                        op: BinOp::Add,
                        left: Box::new(Expr::Ident("total".into())),
                        right: Box::new(Expr::Int(1)),
                    },
                },
            ]
        );
    }

    #[test]
    fn bare_ident_expression_statement_is_not_mistaken_for_assignment() {
        // `foo()` starts with an Ident but isn't `IDENT "=" ...` -- must stay an expr_stmt.
        assert_eq!(
            parse("foo()"),
            vec![Stmt::ExprStmt(Expr::Call {
                callee: Box::new(Expr::Ident("foo".into())),
                args: vec![],
            })]
        );
    }

    #[test]
    fn parses_var_decl_with_float() {
        assert_eq!(
            parse("var pi = 3.5"),
            vec![Stmt::VarDecl {
                name: "pi".into(),
                value: Expr::Float(3.5),
            }]
        );
    }

    #[test]
    fn parses_bool_and_null_literals() {
        assert_eq!(
            parse("var a = true\nvar b = false\nvar c = null"),
            vec![
                Stmt::VarDecl {
                    name: "a".into(),
                    value: Expr::Bool(true),
                },
                Stmt::VarDecl {
                    name: "b".into(),
                    value: Expr::Bool(false),
                },
                Stmt::VarDecl {
                    name: "c".into(),
                    value: Expr::Null,
                },
            ]
        );
    }

    #[test]
    fn parses_list_literal() {
        assert_eq!(
            parse("var tags = [1, 2, 3]"),
            vec![Stmt::VarDecl {
                name: "tags".into(),
                value: Expr::List(vec![Expr::Int(1), Expr::Int(2), Expr::Int(3)]),
            }]
        );
    }

    #[test]
    fn parses_record_literal() {
        assert_eq!(
            parse(r#"var p = { name: "Alice", age: 30 }"#),
            vec![Stmt::VarDecl {
                name: "p".into(),
                value: Expr::Record(vec![
                    ("name".into(), Expr::Str("Alice".into())),
                    ("age".into(), Expr::Int(30)),
                ]),
            }]
        );
    }

    #[test]
    fn parses_unary_operators() {
        assert_eq!(
            parse("var a = -x\nvar b = !ok"),
            vec![
                Stmt::VarDecl {
                    name: "a".into(),
                    value: Expr::Unary {
                        op: UnaryOp::Neg,
                        expr: Box::new(Expr::Ident("x".into())),
                    },
                },
                Stmt::VarDecl {
                    name: "b".into(),
                    value: Expr::Unary {
                        op: UnaryOp::Not,
                        expr: Box::new(Expr::Ident("ok".into())),
                    },
                },
            ]
        );
    }

    #[test]
    fn respects_operator_precedence() {
        // 1 + 2 * 3 == 7, not 9 -- multiplicative binds tighter than additive.
        let stmts = parse("var x = 1 + 2 * 3");
        assert_eq!(
            stmts,
            vec![Stmt::VarDecl {
                name: "x".into(),
                value: Expr::Binary {
                    op: BinOp::Add,
                    left: Box::new(Expr::Int(1)),
                    right: Box::new(Expr::Binary {
                        op: BinOp::Mul,
                        left: Box::new(Expr::Int(2)),
                        right: Box::new(Expr::Int(3)),
                    }),
                },
            }]
        );
    }

    #[test]
    fn parses_modulo_at_multiplicative_precedence() {
        // 10 - 4 % 3 == 10 - 1 == 9, not (10 - 4) % 3 == 2.
        let stmts = parse("var x = 10 - 4 % 3");
        assert_eq!(
            stmts,
            vec![Stmt::VarDecl {
                name: "x".into(),
                value: Expr::Binary {
                    op: BinOp::Sub,
                    left: Box::new(Expr::Int(10)),
                    right: Box::new(Expr::Binary {
                        op: BinOp::Mod,
                        left: Box::new(Expr::Int(4)),
                        right: Box::new(Expr::Int(3)),
                    }),
                },
            }]
        );
    }

    #[test]
    fn parses_call_and_field_access_chain() {
        // strings.upper(person) -- field access binds, then the result is called.
        let stmts = parse("var x = strings.upper(person)");
        assert_eq!(
            stmts,
            vec![Stmt::VarDecl {
                name: "x".into(),
                value: Expr::Call {
                    callee: Box::new(Expr::FieldAccess {
                        target: Box::new(Expr::Ident("strings".into())),
                        field: "upper".into(),
                    }),
                    args: vec![Expr::Ident("person".into())],
                },
            }]
        );
    }

    #[test]
    fn parses_fn_decl_with_params_and_return_type() {
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
                    type_expr: simple_type("string"),
                }],
                return_type: Some(simple_type("string")),
                body: vec![Stmt::Return(Some(Expr::Binary {
                    op: BinOp::Add,
                    left: Box::new(Expr::Str("Hello, ".into())),
                    right: Box::new(Expr::Ident("person".into())),
                }))],
            }]
        );
    }

    #[test]
    fn parses_generic_return_type() {
        let stmts = parse(
            r#"fn classify(age: int) -> Result<string, string> {
    return Ok("adult")
}"#,
        );
        let Stmt::FnDecl { return_type, .. } = &stmts[0] else {
            panic!("expected FnDecl");
        };
        assert_eq!(
            *return_type,
            Some(TypeExpr {
                name: "Result".into(),
                args: vec![simple_type("string"), simple_type("string")],
            })
        );
    }

    #[test]
    fn parses_bare_return() {
        assert_eq!(
            parse("fn noop() {\n    return\n}"),
            vec![Stmt::FnDecl {
                name: "noop".into(),
                params: vec![],
                return_type: None,
                body: vec![Stmt::Return(None)],
            }]
        );
    }

    #[test]
    fn parses_if_else() {
        let stmts = parse(
            r#"if age >= 18 {
    var x = 1
} else {
    var x = 2
}"#,
        );
        assert_eq!(
            stmts,
            vec![Stmt::If {
                condition: Expr::Binary {
                    op: BinOp::GtEq,
                    left: Box::new(Expr::Ident("age".into())),
                    right: Box::new(Expr::Int(18)),
                },
                then_branch: vec![Stmt::VarDecl {
                    name: "x".into(),
                    value: Expr::Int(1),
                }],
                else_branch: Some(vec![Stmt::VarDecl {
                    name: "x".into(),
                    value: Expr::Int(2),
                }]),
            }]
        );
    }

    #[test]
    fn parses_else_if_chain_as_nested_if() {
        let stmts = parse(
            r#"if a {
    var x = 1
} else if b {
    var x = 2
} else {
    var x = 3
}"#,
        );
        let Stmt::If { else_branch, .. } = &stmts[0] else {
            panic!("expected If");
        };
        let else_branch = else_branch.as_ref().expect("expected else branch");
        assert_eq!(else_branch.len(), 1);
        assert!(matches!(else_branch[0], Stmt::If { .. }));
    }

    #[test]
    fn parses_while_loop() {
        assert_eq!(
            parse("while x < 10 {\n    var x = 1\n}"),
            vec![Stmt::While {
                condition: Expr::Binary {
                    op: BinOp::Lt,
                    left: Box::new(Expr::Ident("x".into())),
                    right: Box::new(Expr::Int(10)),
                },
                body: vec![Stmt::VarDecl {
                    name: "x".into(),
                    value: Expr::Int(1),
                }],
            }]
        );
    }

    #[test]
    fn parses_for_in_loop() {
        assert_eq!(
            parse("for row in customers {\n    var x = 1\n}"),
            vec![Stmt::For {
                var_name: "row".into(),
                iterable: Expr::Ident("customers".into()),
                body: vec![Stmt::VarDecl {
                    name: "x".into(),
                    value: Expr::Int(1),
                }],
            }]
        );
    }

    #[test]
    fn parses_import() {
        assert_eq!(
            parse(r#"import "std/strings""#),
            vec![Stmt::Import("std/strings".into())]
        );
    }

    #[test]
    fn parses_open() {
        assert_eq!(
            parse(r#"open "shop.db""#),
            vec![Stmt::Open("shop.db".into())]
        );
    }

    #[test]
    fn open_requires_a_string_path() {
        let tokens = Lexer::new("open shop")
            .tokenize()
            .expect("lex should succeed");
        assert!(Parser::new(tokens).parse_program().is_err());
    }

    #[test]
    fn parses_select_with_and_without_where() {
        assert_eq!(
            parse("var r = select name from customers"),
            vec![Stmt::VarDecl {
                name: "r".into(),
                value: Expr::Select {
                    columns: vec!["name".into()],
                    table: "customers".into(),
                    where_clause: None,
                },
            }]
        );

        assert_eq!(
            parse("var r = select name, age from customers where age >= 18"),
            vec![Stmt::VarDecl {
                name: "r".into(),
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
    fn parses_ok_err_and_try_operator() {
        assert_eq!(
            parse("var a = Ok(1)\nvar b = Err(\"bad\")\nvar c = risky()?"),
            vec![
                Stmt::VarDecl {
                    name: "a".into(),
                    value: Expr::Ok(Box::new(Expr::Int(1))),
                },
                Stmt::VarDecl {
                    name: "b".into(),
                    value: Expr::Err(Box::new(Expr::Str("bad".into()))),
                },
                Stmt::VarDecl {
                    name: "c".into(),
                    value: Expr::Try(Box::new(Expr::Call {
                        callee: Box::new(Expr::Ident("risky".into())),
                        args: vec![],
                    })),
                },
            ]
        );
    }

    #[test]
    fn reports_error_on_missing_from() {
        let err = parse_err("var x = select name customers");
        assert!(err.contains("From"));
    }

    #[test]
    fn reports_error_on_unclosed_block() {
        let err = parse_err("fn f() {\n    var x = 1\n");
        assert!(err.contains("RBrace") || err.contains("Eof"));
    }

    #[test]
    fn parses_full_kitchen_sink_example() {
        let src = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/kitchen_sink.foxy"
        ))
        .expect("kitchen sink example should exist");
        let stmts = parse(&src);
        assert_eq!(stmts.len(), 10);
        assert!(matches!(stmts[0], Stmt::Import(_)));
        assert!(matches!(
            &stmts[8],
            Stmt::VarDecl {
                value: Expr::Try(_),
                ..
            }
        ));
        assert!(matches!(
            &stmts[9],
            Stmt::VarDecl {
                value: Expr::Select { .. },
                ..
            }
        ));
    }
}

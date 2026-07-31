#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    VarDecl {
        name: String,
        value: Expr,
    },
    FnDecl {
        name: String,
        params: Vec<Param>,
        return_type: Option<String>,
        body: Vec<Stmt>,
    },
    Return(Expr),
    ExprStmt(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub type_name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Ident(String),
    Int(i64),
    Str(String),
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Select {
        columns: Vec<String>,
        table: String,
        where_clause: Option<Box<Expr>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
}

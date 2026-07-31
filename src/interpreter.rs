use std::collections::HashMap;

use crate::ast::{BinOp, Expr, Stmt, UnaryOp};
use crate::value::Value;

type Scope = HashMap<String, Value>;
type ScopeStack = Vec<Scope>;

#[derive(Debug, Clone, PartialEq)]
struct FunctionDef {
    params: Vec<String>,
    body: Vec<Stmt>,
}

/// Unwinding signal threaded through statement/expression evaluation.
/// `Fatal` always halts the whole program; `Return`/`Propagate` are caught
/// at the nearest function-call boundary (or the top of the program, for
/// top-level code).
enum Signal {
    Return(Value),
    Propagate(Value),
    Fatal(String),
}

type SigResult<T> = Result<T, Signal>;

pub struct Interpreter {
    functions: HashMap<String, FunctionDef>,
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

impl Interpreter {
    pub fn new() -> Self {
        Interpreter {
            functions: HashMap::new(),
        }
    }

    /// Runs a whole program in a fresh top-level scope: hoists `fn`
    /// declarations first (so call order doesn't matter), then executes the
    /// remaining top-level statements in order, as if they were the body of
    /// an implicit `main`. This is what `foxy run` uses.
    pub fn run_program(&mut self, program: &[Stmt]) -> Result<(), String> {
        let mut scopes: Vec<HashMap<String, Value>> = vec![HashMap::new()];
        self.run_in_scopes(program, &mut scopes)
    }

    /// Same as [`Self::run_program`], but against a scope stack the caller
    /// owns and can reuse across calls -- what the REPL uses to keep
    /// variables and functions alive between lines.
    pub fn run_in_scopes(
        &mut self,
        program: &[Stmt],
        scopes: &mut Vec<HashMap<String, Value>>,
    ) -> Result<(), String> {
        for stmt in program {
            if let Stmt::FnDecl {
                name, params, body, ..
            } = stmt
            {
                self.functions.insert(
                    name.clone(),
                    FunctionDef {
                        params: params.iter().map(|p| p.name.clone()).collect(),
                        body: body.clone(),
                    },
                );
            }
        }

        for stmt in program {
            if matches!(stmt, Stmt::FnDecl { .. }) {
                continue;
            }
            match self.exec_stmt(stmt, scopes) {
                Ok(()) => {}
                Err(Signal::Return(_)) => break,
                Err(Signal::Propagate(v)) => {
                    return Err(format!("unhandled error propagated to top level: {v}"));
                }
                Err(Signal::Fatal(msg)) => return Err(msg),
            }
        }
        Ok(())
    }

    /// Evaluates a single expression against a persistent scope stack and
    /// returns its value, for the REPL's "bare expression prints its value"
    /// behavior. `return`/`?` at this level (outside any function) are
    /// reported as errors rather than silently doing something surprising.
    pub fn eval_repl_expr(
        &mut self,
        expr: &Expr,
        scopes: &mut Vec<HashMap<String, Value>>,
    ) -> Result<Value, String> {
        match self.eval_expr(expr, scopes) {
            Ok(v) => Ok(v),
            Err(Signal::Fatal(msg)) => Err(msg),
            Err(Signal::Propagate(v)) => {
                Err(format!("unhandled error propagated to top level: {v}"))
            }
            Err(Signal::Return(_)) => Err("'return' is not valid outside a function".to_string()),
        }
    }

    fn exec_block(&mut self, stmts: &[Stmt], scopes: &mut ScopeStack) -> SigResult<()> {
        scopes.push(Scope::new());
        let result = self.exec_stmts(stmts, scopes);
        scopes.pop();
        result
    }

    fn exec_stmts(&mut self, stmts: &[Stmt], scopes: &mut ScopeStack) -> SigResult<()> {
        for stmt in stmts {
            self.exec_stmt(stmt, scopes)?;
        }
        Ok(())
    }

    fn exec_stmt(&mut self, stmt: &Stmt, scopes: &mut ScopeStack) -> SigResult<()> {
        match stmt {
            Stmt::VarDecl { name, value } => {
                let v = self.eval_expr(value, scopes)?;
                scopes
                    .last_mut()
                    .expect("scope stack is never empty")
                    .insert(name.clone(), v);
                Ok(())
            }
            Stmt::Assign { name, value } => {
                let v = self.eval_expr(value, scopes)?;
                for scope in scopes.iter_mut().rev() {
                    if let Some(slot) = scope.get_mut(name) {
                        *slot = v;
                        return Ok(());
                    }
                }
                Err(Signal::Fatal(format!(
                    "cannot assign to undefined variable '{name}'"
                )))
            }
            Stmt::FnDecl { .. } => Ok(()), // already hoisted
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond = self.eval_bool(condition, scopes)?;
                if cond {
                    self.exec_block(then_branch, scopes)
                } else if let Some(else_branch) = else_branch {
                    self.exec_block(else_branch, scopes)
                } else {
                    Ok(())
                }
            }
            Stmt::While { condition, body } => {
                while self.eval_bool(condition, scopes)? {
                    self.exec_block(body, scopes)?;
                }
                Ok(())
            }
            Stmt::For {
                var_name,
                iterable,
                body,
            } => {
                let items = match self.eval_expr(iterable, scopes)? {
                    Value::List(items) => items,
                    other => {
                        return Err(Signal::Fatal(format!(
                            "'for ... in' expects a list, got {other}"
                        )));
                    }
                };
                for item in items {
                    scopes.push(Scope::from([(var_name.clone(), item)]));
                    let result = self.exec_stmts(body, scopes);
                    scopes.pop();
                    result?;
                }
                Ok(())
            }
            Stmt::Return(expr) => {
                let v = match expr {
                    Some(e) => self.eval_expr(e, scopes)?,
                    None => Value::Null,
                };
                Err(Signal::Return(v))
            }
            Stmt::Import(_) => Ok(()), // module resolution is out of scope until Den (Milestone 4)
            Stmt::ExprStmt(expr) => {
                self.eval_expr(expr, scopes)?;
                Ok(())
            }
        }
    }

    fn eval_bool(&mut self, expr: &Expr, scopes: &mut ScopeStack) -> SigResult<bool> {
        match self.eval_expr(expr, scopes)? {
            Value::Bool(b) => Ok(b),
            other => Err(Signal::Fatal(format!(
                "condition must be a bool, got {other}"
            ))),
        }
    }

    fn eval_expr(&mut self, expr: &Expr, scopes: &mut ScopeStack) -> SigResult<Value> {
        match expr {
            Expr::Int(n) => Ok(Value::Int(*n)),
            Expr::Float(n) => Ok(Value::Float(*n)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Null => Ok(Value::Null),
            Expr::Ident(name) => {
                for scope in scopes.iter().rev() {
                    if let Some(v) = scope.get(name) {
                        return Ok(v.clone());
                    }
                }
                Err(Signal::Fatal(format!("undefined variable '{name}'")))
            }
            Expr::List(elements) => {
                let mut values = Vec::with_capacity(elements.len());
                for e in elements {
                    values.push(self.eval_expr(e, scopes)?);
                }
                Ok(Value::List(values))
            }
            Expr::Record(fields) => {
                let mut values = Vec::with_capacity(fields.len());
                for (name, e) in fields {
                    values.push((name.clone(), self.eval_expr(e, scopes)?));
                }
                Ok(Value::Record(values))
            }
            Expr::Unary { op, expr } => {
                let v = self.eval_expr(expr, scopes)?;
                match (op, v) {
                    (UnaryOp::Neg, Value::Int(n)) => Ok(Value::Int(-n)),
                    (UnaryOp::Neg, Value::Float(n)) => Ok(Value::Float(-n)),
                    (UnaryOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
                    (op, other) => Err(Signal::Fatal(format!(
                        "unary {op:?} is not defined for {other}"
                    ))),
                }
            }
            Expr::Binary { op, left, right } => {
                let l = self.eval_expr(left, scopes)?;
                let r = self.eval_expr(right, scopes)?;
                eval_binary(*op, l, r).map_err(Signal::Fatal)
            }
            Expr::Call { callee, args } => self.eval_call(callee, args, scopes),
            Expr::FieldAccess { target, field } => match self.eval_expr(target, scopes)? {
                Value::Record(fields) => fields
                    .into_iter()
                    .find(|(name, _)| name == field)
                    .map(|(_, v)| v)
                    .ok_or_else(|| Signal::Fatal(format!("record has no field '{field}'"))),
                other => Err(Signal::Fatal(format!(
                    "cannot access field '{field}' on {other}"
                ))),
            },
            Expr::Try(inner) => match self.eval_expr(inner, scopes)? {
                Value::Result(Ok(v)) => Ok(*v),
                Value::Result(Err(v)) => Err(Signal::Propagate(*v)),
                other => Err(Signal::Fatal(format!(
                    "'?' can only be used on a Result value, got {other}"
                ))),
            },
            Expr::Ok(inner) => Ok(Value::Result(Ok(Box::new(self.eval_expr(inner, scopes)?)))),
            Expr::Err(inner) => Ok(Value::Result(Err(Box::new(self.eval_expr(inner, scopes)?)))),
            Expr::Select { .. } => Err(Signal::Fatal(
                "select expressions require the data engine, which doesn't exist yet (Milestone 2)"
                    .to_string(),
            )),
        }
    }

    fn eval_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        scopes: &mut ScopeStack,
    ) -> SigResult<Value> {
        let Expr::Ident(name) = callee else {
            return Err(Signal::Fatal(
                "only calling a plain function name is supported right now".to_string(),
            ));
        };

        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.eval_expr(arg, scopes)?);
        }

        if let Some(result) = call_builtin(name, &values) {
            return result;
        }

        let Some(func) = self.functions.get(name).cloned() else {
            return Err(Signal::Fatal(format!("undefined function '{name}'")));
        };

        if func.params.len() != values.len() {
            return Err(Signal::Fatal(format!(
                "function '{name}' expects {} argument(s), got {}",
                func.params.len(),
                values.len()
            )));
        }

        let mut fn_scopes: ScopeStack = vec![func.params.into_iter().zip(values).collect()];
        match self.exec_stmts(&func.body, &mut fn_scopes) {
            Ok(()) => Ok(Value::Null),
            Err(Signal::Return(v)) => Ok(v),
            Err(Signal::Propagate(v)) => Ok(Value::Result(Err(Box::new(v)))),
            Err(Signal::Fatal(msg)) => Err(Signal::Fatal(msg)),
        }
    }
}

/// Built-in functions available without any `import`. Returns `None` if
/// `name` isn't one, so the caller falls through to user-defined functions.
fn call_builtin(name: &str, args: &[Value]) -> Option<SigResult<Value>> {
    match name {
        "print" => {
            let rendered = args
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            println!("{rendered}");
            Some(Ok(Value::Null))
        }
        "read_file" => Some(builtin_read_file(args)),
        "write_file" => Some(builtin_write_file(args)),
        _ => None,
    }
}

fn builtin_read_file(args: &[Value]) -> SigResult<Value> {
    let [Value::Str(path)] = args else {
        return Err(Signal::Fatal(
            "read_file(path) expects a single string argument".to_string(),
        ));
    };
    let result = match std::fs::read_to_string(path) {
        Ok(contents) => Ok(Box::new(Value::Str(contents))),
        Err(e) => Err(Box::new(Value::Str(e.to_string()))),
    };
    Ok(Value::Result(result))
}

fn builtin_write_file(args: &[Value]) -> SigResult<Value> {
    let [Value::Str(path), Value::Str(contents)] = args else {
        return Err(Signal::Fatal(
            "write_file(path, contents) expects two string arguments".to_string(),
        ));
    };
    let result = match std::fs::write(path, contents) {
        Ok(()) => Ok(Box::new(Value::Null)),
        Err(e) => Err(Box::new(Value::Str(e.to_string()))),
    };
    Ok(Value::Result(result))
}

fn eval_binary(op: BinOp, left: Value, right: Value) -> Result<Value, String> {
    use Value::{Bool, Float, Int, Str};

    if op == BinOp::Add
        && let (Str(a), Str(b)) = (&left, &right)
    {
        return Ok(Str(format!("{a}{b}")));
    }

    match op {
        BinOp::Eq => return Ok(Bool(values_equal(&left, &right))),
        BinOp::NotEq => return Ok(Bool(!values_equal(&left, &right))),
        _ => {}
    }

    // Int op Int stays Int; any Float operand promotes the whole expression
    // to Float. Anything else (strings, bools, ...) is a type error here.
    match (left, right) {
        (Int(a), Int(b)) => int_binary(op, a, b),
        (Int(a), Float(b)) => float_binary(op, a as f64, b),
        (Float(a), Int(b)) => float_binary(op, a, b as f64),
        (Float(a), Float(b)) => float_binary(op, a, b),
        (l, r) => Err(format!("{op:?} is not defined for {l} and {r}")),
    }
}

fn int_binary(op: BinOp, a: i64, b: i64) -> Result<Value, String> {
    use Value::{Bool, Int};
    match op {
        BinOp::Add => Ok(Int(a + b)),
        BinOp::Sub => Ok(Int(a - b)),
        BinOp::Mul => Ok(Int(a * b)),
        BinOp::Div => {
            if b == 0 {
                Err("division by zero".to_string())
            } else {
                Ok(Int(a / b))
            }
        }
        BinOp::Mod => {
            if b == 0 {
                Err("division by zero".to_string())
            } else {
                Ok(Int(a % b))
            }
        }
        BinOp::Lt => Ok(Bool(a < b)),
        BinOp::LtEq => Ok(Bool(a <= b)),
        BinOp::Gt => Ok(Bool(a > b)),
        BinOp::GtEq => Ok(Bool(a >= b)),
        BinOp::Eq | BinOp::NotEq => unreachable!("handled by values_equal"),
    }
}

fn float_binary(op: BinOp, a: f64, b: f64) -> Result<Value, String> {
    use Value::{Bool, Float};
    match op {
        BinOp::Add => Ok(Float(a + b)),
        BinOp::Sub => Ok(Float(a - b)),
        BinOp::Mul => Ok(Float(a * b)),
        BinOp::Div => {
            if b == 0.0 {
                Err("division by zero".to_string())
            } else {
                Ok(Float(a / b))
            }
        }
        BinOp::Mod => {
            if b == 0.0 {
                Err("division by zero".to_string())
            } else {
                Ok(Float(a % b))
            }
        }
        BinOp::Lt => Ok(Bool(a < b)),
        BinOp::LtEq => Ok(Bool(a <= b)),
        BinOp::Gt => Ok(Bool(a > b)),
        BinOp::GtEq => Ok(Bool(a >= b)),
        BinOp::Eq | BinOp::NotEq => unreachable!("handled by values_equal"),
    }
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(x), Value::Float(y)) | (Value::Float(y), Value::Int(x)) => *x as f64 == *y,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn run(src: &str) -> Result<(), String> {
        let tokens = Lexer::new(src).tokenize().expect("lex should succeed");
        let program = Parser::new(tokens)
            .parse_program()
            .expect("parse should succeed");
        Interpreter::new().run_program(&program)
    }

    /// Runs a program whose final top-level statement is `var result = <expr>`,
    /// and returns the value bound to `result`.
    fn eval_result(src: &str) -> Value {
        let full = format!("{src}\nvar __probe_result = result");
        let tokens = Lexer::new(&full).tokenize().expect("lex should succeed");
        let program = Parser::new(tokens)
            .parse_program()
            .expect("parse should succeed");

        let mut interp = Interpreter::new();
        for stmt in &program {
            if let Stmt::FnDecl {
                name, params, body, ..
            } = stmt
            {
                interp.functions.insert(
                    name.clone(),
                    FunctionDef {
                        params: params.iter().map(|p| p.name.clone()).collect(),
                        body: body.clone(),
                    },
                );
            }
        }
        let mut scopes: ScopeStack = vec![Scope::new()];
        for stmt in &program {
            if matches!(stmt, Stmt::FnDecl { .. }) {
                continue;
            }
            match interp.exec_stmt(stmt, &mut scopes) {
                Ok(()) => {}
                Err(Signal::Fatal(msg)) => panic!("runtime error: {msg}"),
                Err(Signal::Propagate(v)) => panic!("unhandled propagation: {v}"),
                Err(Signal::Return(_)) => break,
            }
        }
        scopes
            .last()
            .unwrap()
            .get("__probe_result")
            .cloned()
            .expect("__probe_result should be set")
    }

    #[test]
    fn evaluates_arithmetic_with_int_float_promotion() {
        assert_eq!(eval_result("var result = 1 + 2"), Value::Int(3));
        assert_eq!(eval_result("var result = 1 + 2.5"), Value::Float(3.5));
        assert_eq!(eval_result("var result = 2.5 + 1"), Value::Float(3.5));
    }

    #[test]
    fn integer_division_and_modulo_stay_integer() {
        assert_eq!(eval_result("var result = 7 / 2"), Value::Int(3));
        assert_eq!(eval_result("var result = 7 % 2"), Value::Int(1));
    }

    #[test]
    fn division_by_zero_is_a_runtime_error() {
        let err = run("var x = 1 / 0").unwrap_err();
        assert!(err.contains("division by zero"));
    }

    #[test]
    fn string_concatenation_with_plus() {
        assert_eq!(
            eval_result(r#"var result = "Hello, " + "world""#),
            Value::Str("Hello, world".into())
        );
    }

    #[test]
    fn comparisons_and_equality() {
        assert_eq!(eval_result("var result = 1 == 1.0"), Value::Bool(true));
        assert_eq!(eval_result("var result = 3 > 2"), Value::Bool(true));
        assert_eq!(eval_result(r#"var result = 1 == "1""#), Value::Bool(false));
    }

    #[test]
    fn if_else_branches() {
        // `var` inside a branch is block-scoped and doesn't leak out -- `result`
        // has to be declared outside and reassigned inside to observe it here.
        assert_eq!(
            eval_result("var result = 0\nif true {\n    result = 1\n} else {\n    result = 2\n}"),
            Value::Int(1)
        );
    }

    #[test]
    fn while_loop_accumulates() {
        let src = "var total = 0\nvar i = 1\nwhile i <= 5 {\n    total = total + i\n    i = i + 1\n}\nvar result = total";
        assert_eq!(eval_result(src), Value::Int(15));
    }

    #[test]
    fn for_in_loop_over_list() {
        let src =
            "var total = 0\nfor n in [1, 2, 3, 4] {\n    total = total + n\n}\nvar result = total";
        assert_eq!(eval_result(src), Value::Int(10));
    }

    #[test]
    fn function_call_and_return() {
        let src = "fn double(n: int) -> int {\n    return n * 2\n}\nvar result = double(21)";
        assert_eq!(eval_result(src), Value::Int(42));
    }

    #[test]
    fn function_without_explicit_return_yields_null() {
        let src = "fn noop() {\n    var x = 1\n}\nvar result = noop()";
        assert_eq!(eval_result(src), Value::Null);
    }

    #[test]
    fn try_operator_unwraps_ok_inside_function() {
        let src = "fn safe_div(a: int, b: int) -> Result<int, string> {\n    if b == 0 {\n        return Err(\"div by zero\")\n    }\n    return Ok(a / b)\n}\nfn double_safe(a: int, b: int) -> int {\n    var v = safe_div(a, b)?\n    return v * 2\n}\nvar result = double_safe(10, 2)";
        assert_eq!(eval_result(src), Value::Int(10));
    }

    #[test]
    fn try_operator_propagates_err_as_function_result() {
        let src = "fn safe_div(a: int, b: int) -> Result<int, string> {\n    if b == 0 {\n        return Err(\"div by zero\")\n    }\n    return Ok(a / b)\n}\nfn double_safe(a: int, b: int) -> Result<int, string> {\n    var v = safe_div(a, b)?\n    return Ok(v * 2)\n}\nvar result = double_safe(10, 0)";
        assert_eq!(
            eval_result(src),
            Value::Result(Err(Box::new(Value::Str("div by zero".into()))))
        );
    }

    #[test]
    fn try_operator_at_top_level_halts_with_the_wrapped_error() {
        let src = "fn always_fails() -> Result<int, string> {\n    return Err(\"nope\")\n}\nvar x = always_fails()?";
        let err = run(src).unwrap_err();
        assert!(err.contains("nope"));
    }

    #[test]
    fn undefined_variable_is_a_runtime_error() {
        let err = run("var x = y").unwrap_err();
        assert!(err.contains("undefined variable"));
    }

    #[test]
    fn assign_to_undeclared_variable_is_a_runtime_error() {
        let err = run("x = 1").unwrap_err();
        assert!(err.contains("undefined variable"));
    }

    #[test]
    fn record_field_access() {
        assert_eq!(
            eval_result(
                r#"var p = { name: "Alice", age: 30 }
var result = p.age"#
            ),
            Value::Int(30)
        );
    }

    #[test]
    fn print_builtin_does_not_error() {
        assert!(run(r#"print("hello", 1, true)"#).is_ok());
    }

    #[test]
    fn runs_fizzbuzz_example() {
        let src = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/fizzbuzz.foxy"
        ))
        .expect("fizzbuzz example should exist");
        assert!(run(&src).is_ok());
    }

    #[test]
    fn write_file_then_read_file_round_trips_via_result() {
        let path = std::env::temp_dir().join("foxy_interp_test_round_trip.txt");
        let path_str = path.to_str().unwrap().replace('\\', "\\\\");
        let src = format!(
            r#"var w = write_file("{path_str}", "hello from foxy")?
var result = read_file("{path_str}")?"#
        );
        assert_eq!(eval_result(&src), Value::Str("hello from foxy".into()));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_file_missing_path_is_an_err_variant() {
        match eval_result(r#"var result = read_file("/no/such/path/foxy_missing.txt")"#) {
            Value::Result(Err(_)) => {}
            other => panic!("expected an Err Result, got {other:?}"),
        }
    }

    #[test]
    fn runs_file_io_example() {
        let src = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/file_io.foxy"
        ))
        .expect("file_io example should exist");
        assert!(run(&src).is_ok());
    }
}

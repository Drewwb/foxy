# Foxy Core Grammar — v0.1 (draft)

Status: **draft, awaiting sign-off**. Covers Milestone 1 (the kernel) only — no table/cursor/`open`/`browse` syntax yet. Those are Milestone 2 decisions and are deliberately not made here.

This doc is the thing we implement against. If the parser and this doc disagree, one of them is wrong — say so before writing more code.

## Lexical notes

- Statements are terminated by a newline. No semicolons, not even optional — one obvious way (design doc, Part 4.3).
- Line comments: `// ...`. Block comments: `/* ... */`, nestable.
- Keywords are reserved, lowercase, no abbreviations (design doc, 2.3): `var fn return if else while for in import true false null Result Ok Err`.
- Identifiers: `[a-zA-Z_][a-zA-Z0-9_]*`, case-sensitive.

## Literal types (Milestone 1 scope)

Per your call: int/float split, bool, list & record literals. **Dates/times deferred** — not in this grammar pass, will land as part of Milestone 2 alongside the data engine rather than as a bolt-on stdlib type now.

| Type | Example | Notes |
|---|---|---|
| int | `30`, `-4` | no separate `Int`/`Int64` split yet |
| float | `30.5`, `-0.1` | requires a `.` and at least one digit on each side |
| string | `"Alice"` | double-quoted only; escapes: `\" \\ \n \t` |
| bool | `true`, `false` | |
| null | `null` | the one kind of nothing (design doc, 2.4) |
| list | `[1, 2, 3]` | homogeneous by convention, not yet enforced |
| record | `{ name: "Alice", age: 30 }` | ordered key: value pairs, bare identifier keys |

## Types as written (annotations)

Type annotations appear in `fn` params/return and nowhere else yet (no `var x: int = 5` — inference only for `var`). Built-in type names: `int float string bool` plus `List<T>`, `Result<T, E>` (see below). No user-defined generics in Milestone 1.

## Statements

```ebnf
program        ::= (statement NEWLINE)* EOF

statement       ::= var_decl
                   | assign_stmt
                   | fn_decl
                   | if_stmt
                   | while_stmt
                   | for_stmt
                   | return_stmt
                   | import_stmt
                   | open_stmt
                   | insert_stmt
                   | update_stmt
                   | delete_stmt
                   | expr_stmt

var_decl        ::= "var" IDENT "=" expr

assign_stmt     ::= IDENT "=" expr   (* reassigns an existing binding; no new one is introduced *)

fn_decl         ::= "fn" IDENT "(" params? ")" ("->" type)? block
params          ::= param ("," param)*
param           ::= IDENT ":" type

if_stmt         ::= "if" expr block ("else" (if_stmt | block))?

while_stmt      ::= "while" expr block

for_stmt        ::= "for" IDENT "in" expr block

return_stmt     ::= "return" expr?

import_stmt     ::= "import" STRING

open_stmt       ::= "open" STRING     (* opens a SQLite database file; Milestone 2 *)

insert_stmt     ::= "insert" "into" IDENT expr                                (* expr must be a record *)
update_stmt     ::= "update" IDENT "set" IDENT "=" expr ("," IDENT "=" expr)* "where" expr
delete_stmt     ::= "delete" "from" IDENT "where" expr
                    (* `into` and `set` are contextual: ordinary names everywhere else *)

expr_stmt       ::= expr

block           ::= "{" NEWLINE* (statement NEWLINE)* "}"
```

## Expressions

Precedence, low to high (matches the spike, extended):

```ebnf
expr            ::= try_expr
try_expr        ::= comparison "?"?              (* postfix, only valid on Result-typed exprs *)
comparison      ::= additive (("==" | "!=" | "<" | "<=" | ">" | ">=") additive)*
additive        ::= multiplicative (("+" | "-") multiplicative)*
multiplicative  ::= unary (("*" | "/" | "%") unary)*
unary           ::= ("-" | "!")? call
call            ::= primary ("(" args? ")" | "." IDENT)*
primary         ::= INT | FLOAT | STRING | "true" | "false" | "null"
                   | IDENT
                   | list_lit
                   | record_lit
                   | "(" expr ")"
                   | select_expr
                   | "Ok" "(" expr ")"
                   | "Err" "(" expr ")"

list_lit        ::= "[" (expr ("," expr)*)? "]"
record_lit      ::= "{" (record_field ("," record_field)*)? "}"
record_field    ::= IDENT ":" expr

select_expr     ::= "select" IDENT ("," IDENT)* "from" IDENT ("where" expr)?
```

`select` stays exactly as spiked — it's already core grammar, not deferred to Milestone 2. What *is* deferred: `open table`, `into cursor`, and insert/update/delete statements, none of which are needed to parse a `select` expression.

**Milestone 2 decisions (Drew, Nov 2026):** a program opens a whole database with `open "shop.db"` (not `open table`), and every table in it is then usable by name. `select` returns a list of records, fields in the order selected; a dedicated cursor type can come later. `where` is evaluated by the interpreter row by row, not translated to SQL. Opening a file that doesn't exist is an error (no empty database is created). Inside `where`, every column of the current row is in scope by name (selected or not), and columns shadow variables of the same name, as in SQL. Column names match case-insensitively, like SQLite.

**Cursors (query a query):** the name after `from` is first looked up as a variable; if one is in scope it must hold a list of records, and it is queried instead of a database table (no `open` needed). A variable wins over a table of the same name. A record missing a field reads as `null` for it; selecting from an empty list returns an empty list.

**Writes (Drew, Nov 2026):** SQL-like `insert into` / `update ... set ... where` / `delete from ... where`, always against a database table (never a variable). `update` and `delete` must have a `where` clause — a parse error otherwise — and `where true` means every row, so a forgotten clause can't wipe a table. `where` and `set` expressions are evaluated per row by the interpreter, with the row's columns in scope (so `set age = age + 1` works). All rows are evaluated first, then every change is written in one SQLite transaction: if anything fails, nothing changes. Values stored: int, float, string, null; bools as 1/0; lists, records, and Results are rejected.

**REPL grids (Drew, Nov 2026):** in the REPL only, a bare expression whose value is a non-empty list of records prints as a text grid with a row count. `print()` keeps the one-line form everywhere, so script output stays predictable. Stored `null` shows as `null`; a field a record lacks shows blank; long cells are cut at 40 characters.

## Functions, `Result`, and `?`

- `Result<T, E>` is a built-in two-parameter type, not user-defined generics.
- `Ok(expr)` and `Err(expr)` are constructor calls recognized as primaries (not general function calls — `Ok`/`Err` are reserved).
- `?` is postfix on any expression; only meaningful where the enclosing function returns `Result<_, E>`. The grammar accepts it anywhere (parser doesn't check types); a later type-checking pass rejects misuse. This keeps the parser's job purely syntactic.

Example:

```foxy
fn open_file(path: string) -> Result<string, string> {
    var contents = try_open(path)?
    return Ok(contents)
}
```

## Modules

```foxy
import "std/strings"

var shout = strings.upper("hi")
```

The bound name is the last path segment (`std/strings` → `strings`). Resolution itself (Den, file lookup) is out of scope for Milestone 1 — the grammar just needs to parse `import STRING` and dotted member access (`call ::= primary "." IDENT` above already covers `strings.upper`).

## Worked example (kitchen sink)

```foxy
import "std/strings"

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

fn sum_ages(people: List<int>) -> int {
    var total = 0
    for age in people {
        total = total + age
    }
    return total
}

var label = classify(age)?
var adults = select name, age from customers where age >= 18
```

## Open questions I made a call on (flag if you want to change any)

- **Added `%` (modulo) at multiplicative precedence.** Same story as `assign_stmt` below: FizzBuzz — the Milestone 1 exit-criteria example — is impossible to write without it, and every C-family/scripting language places it alongside `*`/`/`, so this isn't really a fork worth a question.
- **Added `assign_stmt` (bare `IDENT "=" expr`) for reassignment**, distinct from `var_decl`. This wasn't in the original v0.1 draft you signed off on — it surfaced as a real gap while implementing the parser: without it, no loop can accumulate a running total (`sum_ages` in the kitchen-sink example needs `total = total + age`). This mirrors how Go/Rust/Kotlin/Swift all work (`var`/`let` declares, bare `=` reassigns), so I added it rather than re-asking, but flagging since it's a grammar change post-signoff.
- Block comments `/* */` added for completeness — wasn't asked, low-risk default.
- `null` as the keyword (vs `nil`/`none`) — matches "one kind of nothing" without importing Rust's `Option<T>` machinery.
- `else if` chains via the recursive `if_stmt` in `else`, not a separate `elif` keyword.
- No exponent operator, no bitwise operators, no ternary — not mentioned in the design doc, kept out until something needs them.
- `List<T>` written as a type but list *literals* don't currently infer/check element type — type-checking is out of scope for the parser itself.
- **Known gap, not yet fixed:** `null` cannot appear in type position (e.g. `Result<null, string>` fails to parse — `null` lexes as the literal keyword, not a type name), so there's no clean way to write "a Result whose success case carries nothing." Worked around in `examples/file_io.foxy` by having the success payload be a real value instead. Needs a decision (a `Unit`/`Null` type name? allow bare `null` as a type?) before it's exercised for real — not deciding it under time pressure just to unblock an example.

---

Once this is signed off, the plan is: extend the spike's lexer/parser into `src/` against this grammar, in slices (lexer first with full token coverage + tests, then statement-level parsing, then expressions/`select`/`Result`/`?`), pausing after each for you to look at real output before I continue.

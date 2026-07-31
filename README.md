# Foxy 🦊

*SQLite's embeddability + FoxPro's data-language fusion + modern tooling.*

**Status:** early days — Milestone 0/1 (kernel: lexer, parser). Nothing here runs real programs yet.

```
cargo build
cargo test
cargo run                         # starts the REPL
cargo run -- run examples/fizzbuzz.foxy
cargo run -- fmt some/file.foxy   # placeholder, not yet implemented
```

---

# Foxy — Language Design Document & Phase One Roadmap

**A modern, data-native language inspired by Visual FoxPro. Not a port. Not backward compatible. A successor in spirit.**

- **Language:** Foxy
- **Package manager:** Den
- **One-line pitch:** *SQLite's embeddability + FoxPro's data-language fusion + modern tooling.*
- **Target developer:** The "departmental developer" — someone who needs to go from *"boss needs a tool to track inspections"* to a working, deployed app in days, without an ORM, a JS framework, and a Kubernetes cluster.

---

## Part 1 — What We Keep (Foxy's Identity)

These are the ideas from FoxPro that remain genuinely unmatched today. They are the reason Foxy exists.

### 1.1 The data-native core (the crown jewel)
- Tables, queries, and result sets are **language primitives**, not library calls.
- A dataset is as native to Foxy as an array is to Python: no ORM, no connection strings, no driver ceremony, no impedance mismatch.
- Opening a table and querying it should be one line of ordinary code.
- **Design goal:** a newcomer's "hello world" for data is under 3 lines.

### 1.2 Cursors, reborn as first-class dataframes
- VFP's cursors were dataframes 20 years before pandas: lightweight, in-memory, queryable result sets.
- In Foxy, **everything is re-queryable** — including query results. Query a query. Index it. Bind UI to it.
- This unifies the "database language" and "data analysis language" worlds in one primitive.

### 1.3 The interactive, REPL-driven experience
- VFP's Command Window was Jupyter before Jupyter: poke at live data, test a query, tweak, iterate.
- Foxy ships with a first-class REPL as the *default* way to explore data — not an afterthought.
- The REPL, the script runner, and the compiler share the same engine and semantics.

### 1.4 Radical practicality for CRUD & business apps
- Preserve the *spirit* of `CREATE FORM`-level productivity: instant data-bound interfaces from a single command.
- Modern form: `browse customers` opens a live, editable data grid (browser tab or TUI) — the 2026 answer to `BROWSE`.
- Foxy optimizes for the underserved middle ground between Excel abuse and overengineered web stacks.

### 1.5 Deployment simplicity
- A VFP app was an EXE and some data files. Foxy honors that:
  - Compiles to a **single static binary** (Go-style).
  - Data engine **embedded** in the runtime (SQLite-style).
  - "Copy the file, run the app" remains a valid deployment strategy.

### 1.6 Table schemas as types
- A revamp that turns a VFP weakness into a flagship feature:
  - The compiler **knows your table's shape**.
  - A typo'd column name is a **compile-time error**, not a runtime surprise.
- No mainstream dynamic scripting language offers this. Foxy leads with it.

---

## Part 2 — What We Scrap or Fundamentally Rebuild

Kept in spirit where noted; the implementations do not survive.

### 2.1 The DBF format → replaced as default storage
- **Why it dies:** no real transactions, weak concurrency, corruption-prone, 2GB limits, memo-file fragility.
- **Replacement:** modern embedded storage engine (SQLite-backed initially; possibly custom later).
- **Spirit preserved:** data lives in files you can copy around.
- **Compat concession:** DBF supported as an *import/export* format only — for rescuing legacy data, never for new work.

### 2.2 The Win32 GUI layer → web-native / terminal-native UI
- **Why it dies:** pixel-positioned, single-platform, unsalvageable in a cross-platform world.
- **Replacement:** one command yields a live data-bound UI rendered in a browser or TUI.
- **Spirit preserved:** instant UI from data, zero boilerplate. The *concept* of the form designer survives; the implementation is entirely new.

### 2.3 Loose syntax rules → strict, tooling-friendly grammar
- **Scrapped:** case-insensitive keywords, 4-letter abbreviations (`REPL` for `REPLACE`), macro substitution (`&var`).
- **Why:** these make static analysis, security, LSP servers, and AI-assisted coding unreliable.
- **Replacement:** one unambiguous, canonical syntax. A formatter (like `gofmt`) ships in Phase One so style debates never start.

### 2.4 The type system → gradual/static typing, data-aware
- **Scrapped:** loose typing, NULL-vs-empty ambiguity, date weirdness.
- **Replacement:**
  - Gradual typing: dynamic-feeling in the REPL, strict in compiled code.
  - First-class schema types (see 1.6).
  - Sound, boring, predictable NULL semantics. One kind of nothing.

### 2.5 Single-user, LAN-file assumptions → concurrent by default
- **Scrapped:** the "one user, one machine, shared files on a LAN" worldview.
- **Replacement:**
  - Async as a native language concept, not a bolt-on.
  - Safe multi-writer semantics from day one.
  - Standing up an HTTP server as easy as `USE` was: `serve app on :8080`.

### 2.6 The PRG/SCX/VCX file zoo → one coherent module system
- **Scrapped:** the menagerie of file types and the "share code by emailing PRG files" era.
- **Replacement:** a single module format, resolved and versioned by **Den**.
- Note: Den preserves nothing from VFP — VFP had *no* package ecosystem. Den fills the single biggest hole the original had.

### 2.7 Backward compatibility → explicitly rejected
- No bug-for-bug VFP compatibility. No chasing 1990s semantics.
- Legacy businesses won't migrate anyway; they'll run original VFP until the last Windows box dies.
- If demand appears later, a *separate* transpiler/compat tool can exist outside the core language.
- **Foxy's audience is people who want what VFP felt like — built on foundations from this decade.**

---

## Part 3 — Phase One Roadmap (End-to-End)

**Phase One goal:** a real developer, who is not us, builds and deploys a small data app in Foxy — and enjoys it.

### Milestone 0 — Foundations (Weeks 1–4)
*"Decide everything expensive to change later."*

- [x] Write the **language manifesto** (this doc is the seed) — publish it early; it's also the recruiting tool.
- [x] Choose implementation host language — **Rust**, confirmed after a throwaway lexer/parser spike (see `research/spike-rust/`).
- [ ] Choose storage engine for v0 (**SQLite embedded** is the pragmatic pick; revisit custom engine post-1.0).
- [x] Draft the **core grammar** (EBNF): keywords, expressions, query syntax, module syntax. See [`docs/grammar.md`](docs/grammar.md).
- [ ] Specify the type system philosophy: gradual typing, schema types, NULL semantics — written down *before* coding.
- [x] Set up repo, license (MIT), CI, and a `CONTRIBUTING.md`.
- [ ] Name-check: secure GitHub org, domain, Den registry namespace.

**Exit criteria:** grammar spec v0.1 and typing spec v0.1 merged; toolchain skeleton compiles "hello world."

### Milestone 1 — The Kernel (Weeks 5–12)
*"A language exists."*

- [x] Lexer + parser for core grammar. 65 tests total (lexer 18, parser 24, interpreter 18, REPL 5).
- [x] Tree-walking interpreter first (fast to build, correct-by-construction); compilation comes later.
- [x] Core types: numbers (int/float), strings, bools, lists, records. **Dates/times deferred** to Milestone 2.
- [x] Control flow, functions, error handling (`Result`-style with postfix `?`). **Modules**: `import` parses but resolution is deferred to Den (Milestone 4) — no working module system yet.
- [x] The **REPL v0** — `foxy` with no arguments (or `foxy repl`) drops in. Multi-line blocks, persistent variables/functions across lines, bare expressions print their value.
- [x] Canonical formatter (`foxy fmt`) — placeholder command shipped; real formatting comes later.

**Exit criteria:** FizzBuzz, file I/O, and a 200-line script all run; REPL is pleasant.

### Milestone 2 — The Data Soul (Weeks 13–24)
*"The reason Foxy exists comes alive."*

- [ ] Embed SQLite; design the Foxy-to-storage mapping.
- [ ] Native table syntax: `open table customers`, `select ... into cursor`, insert/update/delete as language statements.
- [ ] **Cursors as first-class values:** re-queryable, indexable, iterable, passable to functions.
- [ ] Schema types: compiler/interpreter validates column names and types against known schemas.
- [ ] DBF **import** tool (`foxy import legacy.dbf`) — the bridge for FoxPro refugees, and a great marketing story.
- [ ] REPL data affordances: pretty-printed result grids, `describe`, quick histograms.

**Exit criteria:** the 3-line data hello-world works; a typo'd column name fails at check time; a real DBF file imports cleanly.

### Milestone 3 — Instant UI (Weeks 25–32)
*"The BROWSE moment."*

- [ ] `browse <cursor>` opens a live, editable data grid in the browser (localhost server auto-spawned).
- [ ] TUI fallback for terminal-only environments.
- [ ] Two-way binding: edits in the grid write back through the same validation as code.
- [ ] Minimal layout primitives: forms with fields, buttons, and actions declared in Foxy — no HTML/CSS/JS required from the user.

**Exit criteria:** a screencast of `open table inspections` → `browse` → editing live data. This is the demo that makes people say "wait, what?" — it should be under 30 seconds.

### Milestone 4 — Den v0 (Weeks 33–40)
*"An ecosystem can begin."*

- [ ] Manifest format (`den.toml`): name, version, deps, entry point.
- [ ] `den add`, `den install`, `den publish`, `den run` — the essential verbs.
- [ ] Simple registry v0 (static index on object storage is fine — crates.io started humbler than you'd think).
- [ ] Semantic versioning + lockfile from day one.
- [ ] Seed the registry: 5–10 first-party packages (HTTP client, CSV, JSON, date utils, testing) so `den search` isn't a ghost town.

**Exit criteria:** a third party publishes a package and someone else installs and uses it.

### Milestone 5 — Ship & Serve (Weeks 41–52)
*"From script to product."*

- [ ] `foxy build` → single static binary with embedded data engine and assets.
- [ ] `serve` primitive: HTTP routes + handlers in idiomatic Foxy; JSON in/out natively.
- [ ] Cross-platform targets: Linux, macOS, Windows.
- [ ] Async foundations exposed in the language (even if minimal at first).
- [ ] Docs site v1: tutorial ("Build an inspection tracker in an afternoon"), language reference, Den guide.
- [ ] **Phase One capstone:** recruit 3–5 outside developers to build real small apps; fix what makes them swear.

**Exit criteria (Phase One complete):** an outside developer builds and deploys a working CRUD app — single binary, embedded data, instant UI — and writes about it voluntarily.

---

## Part 4 — Guiding Principles (Tiebreakers for Every Future Decision)

1. **Data at your fingertips.** If a feature adds distance between the developer and their data, it's wrong.
2. **An app in an afternoon.** Optimize for time-to-working-tool, not architectural purity.
3. **One obvious way.** One syntax, one formatter, one module system. Boring is a feature.
4. **Tooling is the language.** If the LSP, formatter, and AI assistants can't reason about it, redesign it.
5. **Honor the fox, don't embalm it.** Preserve what VFP *felt* like, never what it literally was.

---

## Part 5 — Immediate Next Actions (This Month)

- [ ] Continue the Microsoft/Greg Smith thread — an open sourced VFP becomes the historical reference for Foxy's design research (not its code).
- [x] Publish the manifesto (this document, polished) as the repo's README — it will attract the exact people who miss FoxPro.
- [x] Prototype the 3-line data hello-world on paper: agree on the exact syntax before writing the parser.
- [x] Pick the host language (Rust vs Go) — spike a toy lexer in each for one weekend and choose by feel.
- [ ] Post the idea in xBase communities (Foxite, the VFP subreddit, Harbour/X# forums) — your first contributors are already there, waiting for exactly this.

*The fox is dead. Long live the fox.* 🦊

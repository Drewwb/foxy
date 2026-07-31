# Contributing to Foxy

Foxy is very early — pre-alpha, kernel (lexer/parser) stage. Expect churn.

## Building

```
cargo build
cargo test
```

## Before opening a PR

```
cargo fmt
cargo clippy -- -D warnings
cargo test
```

CI runs all three; a red build won't be merged.

## Scope right now

We're working through [Milestone 1](README.md#milestone-1--the-kernel-weeks-5-12) of the
Phase One roadmap in the README: lexer and parser for the core grammar. Please don't send
PRs that reach ahead into the data engine (Milestone 2) or later milestones — syntax and
semantics are still being decided and getting locked in too early is expensive to undo.

## Syntax decisions

Anything touching Foxy's grammar or keywords is a design decision, not just an
implementation detail. Open an issue to discuss before writing the parser code for it.

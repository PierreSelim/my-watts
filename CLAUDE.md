# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a Rust project that prioritizes:
- **Strong typing** to make illegal states unrepresentable
- **Functional programming** patterns and composability
- **Comprehensive test coverage** for all code
- **Maintainability** through clear specifications and documentation

## Key Documentation Files

The three top-level docs have distinct, non-overlapping roles. Respect them when reading *and* when updating:

1. **README.md** — Short project description, how to build, how to run, what files are produced and where. No exhaustive option tables, no algorithm rationale, no struct definitions. Link to SPEC/ARCHITECTURE for detail.
2. **SPEC.md** — Per-feature specifications: problem, solution, **algorithm choices and rationale**, full CLI option lists, output column definitions, error handling, testing notes. This is the source of truth for *what the tool does and why that approach*.
3. **ARCHITECTURE.md** — Technical design: module layout, key types, data flow, error-handling strategy, future extensibility, testing strategy. This is the source of truth for *how the code is organized*.

### Where each kind of change belongs

| Kind of change | File to update |
|---|---|
| New/renamed CLI flag, default value, or behavior | SPEC.md (CLI Interface section of the relevant feature) + a one-line example in README if it's a common flag |
| New output column, file, or output location | SPEC.md (Output section) + README output table |
| Algorithm change, new algorithm choice, or rationale for picking one | SPEC.md (Algorithm Choice section) — **never** in ARCHITECTURE.md |
| New or renamed module under `src/` | ARCHITECTURE.md module tree |
| New or changed public type/struct/enum | ARCHITECTURE.md "Key Types" |
| New data flow path (e.g. new subcommand pipeline) | ARCHITECTURE.md "Data Flow" |
| New build/test/lint command | README.md "Testing & Code Quality" + Development Commands section below |
| Feature that is designed but not yet implemented | SPEC.md with an explicit "**Status**: planned / partially implemented" banner at the top of the section, listing what is and isn't shipped |

### Rules to keep docs honest

- **No duplication across files.** If something belongs in SPEC, link to it from README rather than copying. If something belongs in ARCHITECTURE, don't restate it in SPEC.
- **Verify against the code before editing.** Defaults, paths, column lists, and type signatures must match `src/` exactly. When in doubt, grep the code.
- **Update docs in the same change as the code.** If a PR changes CLI flags or output, the matching SPEC/README edit is part of that PR, not a follow-up.
- **Mark unimplemented designs explicitly.** A SPEC section without a "planned" banner is a claim that the feature ships today.

## Development Commands

### Building
```bash
cargo build              # Debug build
cargo build --release   # Release build
```

### Testing
```bash
cargo test              # Run all tests
cargo test <test_name>  # Run a specific test
cargo test -- --nocapture  # Run tests with output
cargo test -- --test-threads=1  # Run tests sequentially
```

### Code Quality
```bash
cargo fmt              # Format code with rustfmt (always run before commit)
cargo clippy           # Lint with clippy (address all warnings)
cargo clippy --fix     # Auto-fix clippy warnings where possible
cargo check            # Quick compilation check
```

### Coverage
```bash
cargo tarpaulin --out Html  # Generate test coverage report
```

## Code Style and Architecture Principles

### Type System & Illegal States
- Use types to make illegal states unrepresentable. Prefer `Option<T>` and `Result<T, E>` over boolean flags or sentinel values.
- Leverage Rust's type system to enforce invariants at compile time. If a state is impossible, the type should reflect that.
- Example: prefer `enum Status { Active(Data), Inactive }` over `struct Status { is_active: bool, data: Option<Data> }`.

### Functional Programming
- Prefer pure functions and immutability. Mutation should be localized and justified.
- Use iterator combinators (`map`, `filter`, `fold`, etc.) over imperative loops.
- Avoid deep nesting; use `?` operator for error propagation and consider using function composition.
- Prefer `match` expressions over conditionals for exhaustiveness checking.

### Testing
- All public APIs and complex logic must have unit tests.
- Write tests that verify both happy path and error conditions.
- Use descriptive test names that explain what is being tested and the expected outcome.
- Consider property-based testing for complex logic.

### Linting & Formatting
- All code must pass `cargo fmt` without modifications.
- All code must pass `cargo clippy` without warnings. Suppress warnings only with documented justification.
- Run `cargo test` to ensure no regressions before committing.

## Workflow

1. **Read SPEC.md** to understand what feature or behavior is being implemented.
2. **Check ARCHITECTURE.md** to understand how it fits into the system design.
3. **Plan the types first** — design the data structures and types before writing logic. Strong typing guides implementation.
4. **Implement with types** — let the compiler guide you. If types align, logic often follows.
5. **Write tests** alongside implementation.
6. **Run quality checks** — `cargo fmt`, `cargo clippy`, `cargo test` before marking as complete.
7. If needed update SPEC.md, ARCHITECTURE.md and README.md

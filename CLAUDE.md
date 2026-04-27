# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a Rust project that prioritizes:
- **Strong typing** to make illegal states unrepresentable
- **Functional programming** patterns and composability
- **Comprehensive test coverage** for all code
- **Maintainability** through clear specifications and documentation

## Key Documentation Files

When making decisions or implementing features:
1. **SPEC.md** — Feature specifications and user-facing requirements. Check this before changing behavior.
2. **ARCHITECTURE.md** — Technical decisions, design rationale, and system boundaries. Read this to understand why code is structured as it is.
3. **README.md** — Build commands, run commands, and usage examples. Keep this in sync with actual commands.

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

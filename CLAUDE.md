# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Giallo is a high-performance HTML code highlighter written in Rust that supports 200+ programming languages using TextMate-style grammars. It's designed for syntax highlighting code into HTML output with comprehensive language support.

## Development Commands

### Building and Testing
```bash
# Basic development workflow
cargo build              # Debug build
cargo build --release    # Release build
cargo test               # Run all tests
cargo bench              # Run benchmarks (requires 'dump' feature)

# Feature-specific builds
cargo build --features debug    # Build with logging support
cargo build --features dump     # Build with serialization/compression
cargo build --features tools    # Build with additional tools

# Running specific tests
cargo test <test_name>           # Run specific test
cargo test -- --nocapture       # Show println! output in tests
```

### Registry Management
```bash
# Build grammar registry (requires 'tools' feature)
cargo run --bin build-registry --features tools

# Profile performance
cargo run --example profile_jquery --features dump
```

## Architecture Overview

### Core Components
- **Registry** (`src/registry.rs`): Central component managing grammars, themes, and language definitions
- **Tokenizer** (`src/tokenizer.rs`): Breaks source code into tokens for highlighting
- **Highlighter** (`src/highlight.rs`): Applies themes to tokenized code and generates HTML
- **Scope System** (`src/scope.rs`): Manages TextMate-style scopes for syntax rules
- **Grammar System** (`src/grammars/`): Handles grammar compilation and rule matching

### Language Support
- Grammar files located in `grammars-themes/grammars/` (200+ languages)
- Theme files in `grammars-themes/themes/`
- Uses TextMate grammar format (JSON)
- Language aliases supported via `grammar_metadata.json`

### Key Features
- **Optional Features**: `debug` (logging), `dump` (serialization), `tools` (build utilities), `builtins`
- **Performance**: Uses `onig` regex engine for high-performance pattern matching
- **Output**: Generates HTML with inline styles or CSS classes
- **Extensibility**: Modular design allows custom grammars and themes

## Testing Strategy

- **Unit Tests**: Standard Rust tests with `cargo test`
- **Snapshot Tests**: Uses `insta` crate for regression testing across languages
- **Benchmarks**: Performance testing with `criterion` crate
- **Language Coverage**: Snapshot tests for various programming languages in `/snapshots`

## Known Issues

Current grammar issues documented in README.md:
- XML nesting level issues
- Missing references in some grammars (Jison, MDX, Nextflow)
- Scope resolution problems in Markdown/Wikitext

## Development Notes

- Uses Rust 2024 edition
- Local dependency on custom `rust-onig` fork at `../pulls/rust-onig/onig`
- Release builds include debug symbols for profiling
- Optimized dev builds for `insta` and `similar` crates to speed up testing
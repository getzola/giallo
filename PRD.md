# TextMate Highlighter for Rust - Product Requirements Document

## Executive Summary

A high-performance TextMate grammar-based syntax highlighter written in Rust, designed for static site generators and tools requiring fast, accurate syntax highlighting without JavaScript dependencies. The implementation uses pre-compiled grammars committed to the repository to achieve zero build-time overhead while maintaining compatibility with Visual Studio Code's syntax highlighting ecosystem.

## Project Goals

### Primary Goals
1. **Performance**: Process files at 100+ MB/s with minimal memory allocation
2. **Zero Build Overhead**: Pre-generate and commit all derived data
3. **Compatibility**: Support existing TextMate grammars from the shiki collection
4. **Simple Integration**: Single binary with no external dependencies

### Non-Goals
- Full TextMate editor compatibility (we only need highlighting)
- YAML/plist grammar support (JSON only)
- Real-time editing (batch processing focus)
- Custom theme creation tools
- Runtime grammar compilation

## Architecture Overview

```
giallo/
├── src/
│   ├── lib.rs                 # Public API
│   ├── tokenizer.rs           # Core tokenization logic (98% complete)
│   ├── grammars/              # Grammar system
│   │   ├── mod.rs             # Grammar module exports
│   │   ├── raw.rs             # Grammar loading and parsing
│   │   ├── compiled.rs        # Optimized grammar structures
│   │   └── pattern_set.rs     # PatternSet optimization (RegSet caching)
│   ├── theme.rs               # Theme application and caching
│   └── generated/             # Pre-generated files (committed)
│       └── scopes.rs          # PHF map for scope interning (30K+ scopes)
├── grammars-themes/           # TextMate grammars/themes (git submodule)
│   ├── packages/tm-grammars/  # 238+ language grammars
│   └── packages/tm-themes/    # VSCode themes
└── benches/                   # Performance benchmarks
```

## Core Components

### 1. Pre-Generation Tool (`tools/generate.rs`)

A standalone tool that processes grammars and generates optimized data files. Run manually when grammars change.

#### Responsibilities
- Extract all scope names from grammar files
- Generate PHF map for O(1) scope lookups
- Validate all regex patterns
- Serialize grammars to binary format
- Write generated files to `src/generated/`

#### Output Files
- `scopes.rs`: PHF map with ~10,000 scope mappings
- `grammars.bin`: Binary blob with all grammars (~1MB)

**Usage**: `cargo run --bin tm-generate` (only when grammars change)

### 2. Scope System (`scope.rs`)

#### Requirements
- Include pre-generated PHF map via `include!`
- Convert scope strings to integers in O(1) time
- Fallback HashMap for runtime scopes (rare)
- Incremental hash computation for scope stacks

#### Implementation Details
```rust
// Include pre-generated PHF
include!("generated/scopes.rs");

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ScopeId(pub usize);

pub struct ScopeStack {
    scopes: Vec<ScopeId>,
    hash: u64,  // Pre-computed incrementally
}

pub struct ScopeRegistry {
    // 99% of lookups hit the static PHF
    // 1% fallback to runtime for custom scopes
    runtime_scopes: FxHashMap<String, ScopeId>,
}
```

**Performance Target**: <5ns per scope operation

### 3. Grammar Loading (`grammar.rs`)

#### Binary Embedding
```rust
// Grammars embedded directly in binary
static GRAMMAR_BYTES: &[u8] = include_bytes!("generated/grammars.bin");

// One-time deserialization on first access
pub static GRAMMARS: Lazy<HashMap<String, Grammar>> = Lazy::new(|| {
    bincode::deserialize(GRAMMAR_BYTES).unwrap()
});
```

**Load Time**: <5ms for all grammars (one-time cost)

### 4. Pattern Matching Engine (`pattern.rs`)

#### Optimization Strategy
- Lazy regex compilation (compile on first use)
- Pattern ordering by frequency
- SIMD scanning for plain text regions
- Regex caching with `Arc<Regex>`

#### Implementation
```rust
pub struct CompiledPattern {
    pattern: Pattern,
    regex: OnceCell<Arc<Regex>>, // Lazy compilation
}
```

**Performance Target**: Skip plain text at 1GB/s+

### 5. Tokenizer (`tokenizer.rs`)

#### Core Algorithm
1. Load pre-compiled grammar
2. Process line by line
3. Match patterns and build scope stack
4. Batch consecutive tokens with same style
5. Return minimal token batches

#### Critical Optimizations
- **Token Batching**: Reduce tokens by 10x
- **Style Caching**: 95%+ cache hit rate
- **Zero Allocations**: Reuse buffers

```rust
pub struct TokenBatch {
    pub start: u32,
    pub end: u32,
    pub style: StyleId,
}
```

**Performance Target**: <1ms for 100-character line

### 6. Theme Engine (`theme.rs`)

#### Two-Level Cache Design
```rust
pub struct StyleCache {
    // L1: Last 4 lookups (no hash table overhead)
    recent: [(u64, StyleId); 4],
    // L2: Full cache
    cache: FxHashMap<u64, StyleId>,
}
```

**Cache Target**: >95% hit rate

### 7. HTML Renderer (`renderer.rs`)

#### Batched Output
- Minimal spans (only when style changes)
- Inline styles or CSS classes
- Proper escaping
- Pre-allocated string buffer

## Development Workflow

### Initial Setup
```bash
# Clone with submodules
git clone --recursive https://github.com/org/tm-highlighter

# Generate files (one time)
cargo run --bin tm-generate

# Files are already committed, so this is only needed for updates
```

### Normal Development
```bash
cargo build   # Fast - no build.rs
cargo test    # Fast - uses committed files
cargo bench   # Measure performance
```

### Updating Grammars
```bash
# Update grammar submodule
git submodule update --remote grammars/

# Regenerate files
cargo run --bin tm-generate

# Verify changes
git diff src/generated/

# Commit
git add src/generated/
git commit -m "Update grammars"
```

### CI Pipeline
```yaml
# Verify generated files are up-to-date
- run: cargo run --bin tm-generate
- run: git diff --exit-code src/generated/
```

## Performance Requirements

### Benchmarks
| File Size | Target Time | Throughput |
|-----------|-------------|------------|
| 1 KB      | <10 μs      | 100 MB/s   |
| 10 KB     | <100 μs     | 100 MB/s   |
| 100 KB    | <1 ms       | 100 MB/s   |
| 1 MB      | <10 ms      | 100 MB/s   |

### Memory Requirements
- Binary size increase: ~1.5MB (includes all grammars)
- Runtime memory: <10MB for typical workload
- Per-line overhead: <100 bytes

## Implementation Phases

### Phase 1: Core Engine (Week 1)
- [x] Grammar types and deserialization
- [x] Basic tokenization
- [x] Simple HTML output
- [ ] Integration tests

### Phase 2: Pre-Generation Tool (Week 2)
- [x] Scope extraction tool
- [x] PHF generation
- [x] Binary serialization
- [ ] CI verification

### Phase 3: Optimization (Week 3)
- [x] Token batching
- [x] Style caching (two-level)
- [x] SIMD text scanning
- [ ] Performance benchmarks

### Phase 4: Production (Week 4)
- [ ] Theme support (10+ themes)
- [ ] Error recovery
- [ ] Documentation
- [ ] Zola integration example

## Testing Strategy

### Unit Tests
- Scope interning correctness
- Pattern matching edge cases
- Token batching logic
- Style cache hit rates

### Integration Tests
```rust
#[test]
fn test_all_languages() {
    let highlighter = Highlighter::new();
    for lang in highlighter.list_languages() {
        // Ensure no panics
        let _ = highlighter.highlight("test", lang);
    }
}
```

### Performance Tests
```rust
#[bench]
fn bench_large_file(b: &mut Bencher) {
    let highlighter = Highlighter::new();
    let code = include_str!("fixtures/large.js");
    b.iter(|| highlighter.highlight(code, "source.js"));
}
```

### Snapshot Tests
- Use insta for HTML output comparison
- Verify consistency across grammar updates

## Dependencies

```toml
[dependencies]
onig = "6"                    # TextMate-compatible regex
phf = "0.11"                  # Perfect hash functions
rustc-hash = "2.0"            # Fast hashing
serde = { version = "1.0", features = ["derive"] }
bincode = "1.3"               # Binary serialization
once_cell = "1.19"            # Lazy statics
memchr = "2.7"                # SIMD text scanning

# Note: NO build-dependencies needed!

[dev-dependencies]
criterion = "0.5"             # Benchmarking
insta = "1.39"                # Snapshot testing

# Generator tool dependencies
[[bin]]
name = "tm-generate"
path = "tools/generate.rs"
```

## API Design

```rust
use tm_highlighter::{Highlighter, Theme};

// Simple API
let mut highlighter = Highlighter::new();
let html = highlighter.highlight(code, "rust")?;

// With theme
let mut highlighter = Highlighter::with_theme(Theme::Monokai);
let html = highlighter.highlight(code, "javascript")?;

// Batch processing
let files = vec![("file1.rs", code1), ("file2.js", code2)];
let results = highlighter.highlight_batch(files)?;
```

## File Size Analysis

### Generated Files (committed to repo)
- `scopes.rs`: ~200KB (10,000 scopes as PHF)
- `grammars.bin`: ~1MB (compressed binary)

### Final Binary Size
- Base highlighter: ~500KB
- With embedded data: ~2MB total
- Acceptable for SSG/CLI tools

## Success Metrics

1. **Build Speed**: No build.rs overhead (immediate compilation)
2. **Runtime Performance**: 100+ MB/s throughput
3. **Memory Usage**: <10MB for typical workload
4. **Integration Simplicity**: <10 lines to highlight
5. **Grammar Updates**: <1 minute to regenerate

## Risk Mitigation

| Risk | Mitigation |
|------|------------|
| Generated files out of sync | CI check on every PR |
| Binary size too large | Option to load grammars from disk |
| Regex compilation slow | Lazy compilation, caching |
| Grammar compatibility | Validate during generation |

## Advantages of This Approach

1. **Zero Build Overhead**: No build.rs means instant builds
2. **Reproducible**: Same input → same generated files
3. **Debuggable**: Generated code is visible in repo
4. **IDE Friendly**: rust-analyzer sees all generated code
5. **Publishing Friendly**: Can publish to crates.io
6. **Git Friendly**: Grammar changes visible in diffs

## Open Questions Resolved

1. **Incremental parsing?** → No, batch processing focus
2. **Streaming API?** → No, files fit in memory
3. **Custom grammars?** → Yes, via runtime fallback
4. **HTML format?** → Inline styles by default, CSS classes optional

---

## Summary

This approach prioritizes:
- **Developer experience**: Fast builds, no surprises
- **Performance**: Pre-computed everything possible
- **Simplicity**: Minimal dependencies, clear architecture
- **Maintainability**: Generated files are version controlled

The key insight: **Grammar compilation is rare, builds are frequent**. By pre-generating and committing files, we optimize for the common case (building) rather than the rare case (updating grammars).
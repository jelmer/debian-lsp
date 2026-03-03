# Contributing to debian-lsp

Thank you for your interest in contributing to debian-lsp! This document provides debian-lsp-specific guidelines for contributing.

For general pull request advice, see <https://jelmer.uk/pages/pr-advice.html>

## Performance Philosophy

**Performance is critical for this project.** As a language server, responsiveness directly impacts the developer experience. We must ensure fast response times for all LSP operations.

### Incremental Processing is Key

The project uses [Salsa](https://github.com/salsa-rs/salsa) for incremental computation. The fundamental principle is:

**Process as little of the file as possible when something changes.**

When a user types in their editor, we receive incremental change notifications. Our goal is to:

1. **Parse incrementally**: Only reparse the affected portion of the file
2. **Recompute minimally**: Only update computations that depend on the changed parts
3. **Cache aggressively**: Let Salsa cache intermediate results so unchanged queries return cached values

### Performance Guidelines for Contributors

When adding features or fixing bugs:

1. **Use Salsa queries**: Structure your code as Salsa queries in `workspace.rs` so they benefit from automatic incrementality and caching.

2. **Minimize reparsing**:
   - Don't re-parse entire files on every change
   - Use the lossless parser (`deb822-lossless`) which preserves the parse tree structure
   - Only traverse the relevant parts of the syntax tree

3. **Avoid unnecessary work**:
   - If a change is in a comment, skip analysis that only cares about field values
   - If a change is in one paragraph of `debian/control`, don't reprocess other paragraphs
   - Check the changed range before deciding what to recompute

4. **Think about locality**:
   - Completions only need context around the cursor position
   - Diagnostics for one paragraph don't require analyzing others (in most cases)
   - Hover information only needs the element under the cursor

5. **Profile before optimizing**: If you suspect a performance issue, measure it. The LSP server logs can help identify slow operations.

### Example: Adding a New Diagnostic

```rust
// BAD: Parses the whole file every time
fn get_diagnostics(uri: &Uri, text: &str) -> Vec<Diagnostic> {
    let file = parse_entire_file(text); // expensive!
    analyze_everything(file)
}

// GOOD: Uses Salsa query that caches parse result
#[salsa::tracked]
fn diagnostics(db: &dyn Db, file: File) -> Vec<Diagnostic> {
    let parsed = parse_file(db, file); // cached by Salsa
    analyze_relevant_parts(db, parsed)
}
```

## Development Setup

### Building

```bash
cargo build --release
```

### Running Tests

```bash
cargo test
```

### Code Quality Checks

Before submitting a PR, ensure these pass:

```bash
cargo fmt        # Format code
cargo clippy     # Check for issues
cargo test       # Run tests
```

All CI checks must pass, including formatting, clippy, and tests.

## debian-lsp Specific Conventions

### File Type Detection

File types are detected in `FileType::detect()` based on URI patterns. When adding support for a new file type:

1. Add the variant to `FileType` enum in `src/main.rs`
2. Add detection logic in `FileType::detect()`
3. Create a new module under `src/` for the file type
4. Wire up the handlers in the LSP server implementation

### Parser Integration

We use parsers from the `deb822-lossless` family for most Debian file formats:

- `debian-control` for control files
- `debian-changelog` for changelog files
- `debian-copyright` for copyright files (with `lossless` feature)
- `debian-watch` for watch files
- `deb822-lossless` for generic deb822 files (e.g. `debian/tests/control`)

These parsers preserve all whitespace and formatting, enabling:
- Precise error locations
- Format-preserving edits (quick fixes)
- Incremental reparsing

### Workspace Management

The `Workspace` struct uses Salsa for incremental computation. When adding new LSP features:

1. Add Salsa queries to `workspace.rs` for your computation
2. Use `File` as the input to your queries (represents a document)
3. Let Salsa track dependencies automatically

### Testing

Tests should verify both functionality and error cases. For diagnostics and quick fixes:

- Test that diagnostics are produced at the correct positions
- Test that quick fixes produce the expected edits
- Test edge cases (empty files, malformed input, etc.)

Use `assert_eq!` for exact matching rather than partial checks like `.contains()`.

## Areas for Contribution

- **New file format support**: `debian/upstream/metadata`, `debian/gbp.conf`, etc.
- **Enhanced diagnostics**: Detect more packaging issues
- **Quick fixes**: Automatic corrections for common problems
- **Semantic analysis**: Cross-file checking (e.g., package name consistency)
- **Performance optimization**: Identify and fix bottlenecks
- **Editor integration**: Improve coc-debian and vscode-debian plugins

## Architecture Notes

```
main.rs              # LSP server implementation, message routing
workspace.rs         # Salsa database and incremental queries
control/             # debian/control support
copyright/           # debian/copyright support
watch/               # debian/watch support
changelog/           # debian/changelog support
deb822/              # Common deb822 format utilities
position.rs          # LSP <-> text position conversions
```

The LSP server in `main.rs` handles protocol messages and delegates to file-type-specific modules. The `Workspace` provides incremental computation infrastructure.

## Questions

For questions or bug reports, open an issue on GitHub: <https://github.com/jelmer/debian-lsp>

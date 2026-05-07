//! Module for DEP-3 patch headers.
//!
//! DEP-3 specifies a deb822-shaped header at the top of a quilt patch
//! file under `debian/patches/`. The header runs until the first
//! `---` / `diff ` / `Index:` line — everything after that is the
//! unified diff itself, which we leave to diff-lsp.

pub mod parsing;

pub use parsing::parse_dep3_header;

//! Module for DEP-3 patch headers.
//!
//! DEP-3 specifies a deb822-shaped header at the top of a quilt patch
//! file under `debian/patches/`. The header runs until the first
//! `---` / `diff ` / `Index:` line — everything after that is the
//! unified diff itself, which we leave to diff-lsp.

pub mod completion;
pub mod detection;
pub mod diagnostics;
pub mod fields;
pub mod hover;
pub mod parsing;
pub mod semantic;
#[cfg(feature = "spellcheck")]
pub mod spelling;
pub mod symbols;

pub use completion::get_completions;
pub use detection::is_in_dep3_header;
pub use diagnostics::get_diagnostics;
pub use fields::get_standard_field_name;
pub use hover::get_hover;
#[cfg(any(feature = "lintian-brush", feature = "multiarch-hints"))]
pub use parsing::parse_dep3_header;
pub use semantic::generate_semantic_tokens;
pub use symbols::generate_document_symbols;

//! Module for handling debian/debcargo.toml files.
//!
//! These files configure debcargo, the tool that generates Debian packages
//! from Rust crates. The format is TOML with a fixed set of known keys at
//! the top level, in `[source]`, and in `[packages.KEY]` tables.

pub mod completion;
pub mod detection;
pub mod fields;
pub mod hover;
pub mod semantic;

pub use completion::get_completions;
pub use detection::is_debcargo_toml;
pub use hover::get_hover;
pub use semantic::generate_semantic_tokens;

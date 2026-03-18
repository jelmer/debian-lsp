//! Module for handling debian/rules files.
//!
//! These files are Makefiles that define how to build a Debian package.

pub mod completion;
pub mod detection;
pub mod fields;
pub mod semantic;

pub use completion::get_completions;
pub use detection::is_rules_file;
pub use semantic::generate_semantic_tokens;

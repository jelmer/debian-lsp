//! Module for handling debian/upstream/metadata files (DEP-12).
//!
//! These files use YAML format and contain machine-readable metadata
//! about the upstream project.

pub mod completion;
pub mod detection;
pub mod fields;
pub mod hover;
pub mod semantic;

pub use completion::get_completions;
pub use detection::is_upstream_metadata_file;
pub use hover::get_hover;
pub use semantic::generate_semantic_tokens;

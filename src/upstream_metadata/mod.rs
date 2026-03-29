//! Module for handling debian/upstream/metadata files (DEP-12).
//!
//! These files use YAML format and contain machine-readable metadata
//! about the upstream project.

pub mod completion;
pub mod detection;
pub mod document_link;
pub mod fields;
pub mod hover;
pub mod on_type_formatting;
pub mod semantic;

pub use completion::get_completions;
pub use detection::is_upstream_metadata_file;
pub use document_link::get_document_links;
pub use hover::get_hover;
pub use semantic::generate_semantic_tokens;

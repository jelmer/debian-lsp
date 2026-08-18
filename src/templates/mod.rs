//! Module for debian/templates debconf files.
//!
//! `debian/templates` (and `debian/<package>.templates`) hold the debconf
//! question templates installed by dh_installdebconf. The format is deb822:
//! one paragraph per template with a fixed set of known fields, plus
//! localized `Description-<lang>` / `Choices-<lang>` variants.

pub mod completion;
pub mod detection;
pub mod fields;
pub mod hover;
pub mod semantic;

pub use completion::get_completions;
pub use detection::is_templates_file;
pub use hover::get_hover;
pub use semantic::generate_semantic_tokens;

//! Module for handling debian/conffiles files

pub mod actions;
pub mod completion;
pub mod detection;
pub mod diagnostics;
pub mod hover;
pub mod semantic;
pub use actions::*;
pub use completion::*;
pub use detection::*;
pub use hover::get_hover;
pub use semantic::generate_semantic_tokens;

pub const REMOVE_ON_UPGRADE: &str = "remove-on-upgrade";

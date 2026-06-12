//! Module for handling debian/conffiles files

pub mod completion;
pub mod definition;
pub mod detection;
pub mod fields;
pub mod hover;
pub mod semantic;
pub use completion::*;
pub use definition::goto_definition;
pub use detection::*;
pub use hover::get_hover;
pub use semantic::generate_semantic_tokens;

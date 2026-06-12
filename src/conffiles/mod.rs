//! Module for handling debian/conffiles files

pub mod completion;
pub use hover::get_hover;
pub mod definition;
pub mod detection;
pub mod fields;
pub mod hover;
pub use definition::goto_definition;
pub use detection::*;

pub use completion::*;

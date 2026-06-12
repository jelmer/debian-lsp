//! Module for handling debian/conffiles files

pub mod completion;
pub mod definition;
pub mod detection;
pub mod fields;
pub use definition::goto_definition;
pub use detection::*;

pub use completion::*;

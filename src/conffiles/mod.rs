//! Module for handling debian/conffiles files

pub mod completion;
pub mod detection;
pub mod hover;
pub use completion::*;
pub use detection::*;
pub use hover::get_hover;

pub const REMOVE_ON_UPGRADE: &str = "remove-on-upgrade";

//! Module for handling debian/conffiles files

pub mod completion;
pub mod detection;
pub use detection::*;

pub use completion::*;

pub const REMOVE_ON_UPGRADE: &str = "remove-on-upgrade";

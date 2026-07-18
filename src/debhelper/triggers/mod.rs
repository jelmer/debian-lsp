//! Support for debian/triggers and debian/<package>.triggers files.

pub mod completion;
pub mod detection;

pub use completion::get_completions;
pub use detection::is_triggers_file;

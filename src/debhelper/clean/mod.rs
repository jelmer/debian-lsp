//! Support for debian/clean and debian/<package>.clean files.

pub mod completion;
pub mod detection;

pub use completion::get_completions;
pub use detection::is_clean_file;

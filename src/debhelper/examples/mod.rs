//! Support for debian/examples and debian/<package>.examples files.

pub mod completion;
pub mod detection;

pub use completion::get_completions;
pub use detection::is_examples_file;

//! Support for debian/dirs and debian/<package>.dirs files.

pub mod completion;
pub mod detection;

pub use completion::get_completions;
pub use detection::is_dirs_file;

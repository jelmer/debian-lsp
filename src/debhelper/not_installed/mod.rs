//! Support for debian/not-installed and debian/<package>.not-installed files.

pub mod completion;
pub mod detection;

pub use completion::get_completions;
pub use detection::is_not_installed_file;

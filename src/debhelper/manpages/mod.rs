//! Support for debian/manpages and debian/<package>.manpages files.

pub mod completion;
pub mod detection;

pub use completion::get_completions;
pub use detection::is_manpages_file;

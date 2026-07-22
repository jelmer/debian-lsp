//! Support for debian/install and debian/<package>.install files.

pub mod completion;
pub mod detection;

pub use completion::get_completions;
pub use detection::is_install_file;

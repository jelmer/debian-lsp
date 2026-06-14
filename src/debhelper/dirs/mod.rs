//! Support for debian/dirs and debian/<package>.dirs files.

pub mod completion;
pub mod detection;
pub mod diagnostics;

pub use completion::*;
pub use detection::is_dirs_file;

//! Support for debian/links and debian/<package>.links files.

pub mod completion;
pub mod detection;

pub use completion::get_completions;
pub use detection::{is_links_file, package_dir};

//! Support for debian/install and debian/<package>.install files.

pub mod actions;
pub mod completion;
pub mod detection;
pub mod diagnostics;

pub use actions::get_code_actions;
pub use completion::*;
pub use detection::is_install_file;

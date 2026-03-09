//! Module for handling debian/tests/control files
//!
//! For now, this provides basic file detection and empty completion support.
//! In the future, this will be extended with a dedicated debian-tests crate
//! for proper parsing and validation of autopkgtest control files.

pub mod completion;
pub mod detection;
pub mod semantic;

pub use completion::*;
pub use detection::is_tests_control_file;
pub use semantic::generate_semantic_tokens;

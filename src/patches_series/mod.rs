//! Module for handling debian/patches/series files

pub mod completion;
pub mod definition;
pub mod detection;

pub use completion::*;
pub use detection::{is_patch_file, is_patches_series_file};

pub mod semantic;
pub use definition::goto_definition;
pub use semantic::generate_semantic_tokens;

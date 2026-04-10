//! Module for handling debian/patches/series files

pub mod completion;
pub mod detection;

pub use completion::*;
pub use detection::is_patches_series_file;

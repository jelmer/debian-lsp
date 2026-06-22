//! Support for debhelper-related packaging files.

// Shared building blocks every debhelper helper can reuse. The per-file
// modules below delegate to these so the common logic lives in one place.
pub mod actions;
pub mod completion;
pub mod detection;
pub mod diagnostics;

pub mod dirs;
pub mod install;

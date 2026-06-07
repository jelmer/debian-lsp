//! Emit [SCIP](https://github.com/sourcegraph/scip) indexes for Debian packaging trees.
//!
//! The entry point is [`Indexer`], which walks a `debian/` directory and produces
//! a [`scip::types::Index`] that can be written to disk with
//! [`scip::write_message_to_file`].

// Ported as a self-contained library surface; the binary exercises only part
// of it, while the rest is covered by this module's own tests.
#![allow(dead_code)]

pub mod autopkgtest;
pub mod bug_info;
pub mod changelog;
pub mod control;
pub mod copyright;
pub mod diagnostics;
pub mod highlight;
pub mod indexer;
pub mod linetable;
pub mod patches;
pub mod rules;
pub mod source_format;
pub mod symbols;
pub mod upstream_metadata;
pub mod watch;

#[cfg(test)]
mod tests;

pub use indexer::Indexer;

//! Generic LSP integration for the `debian-workspace` crate.
//!
//! Hosts (lintian-brush, multiarch-hints, ...) program detectors against
//! the `debian_workspace::Workspace` trait and emit
//! `debian_workspace::action::Action` plans. This module provides:
//!
//! * [`workspace::LspDebianWorkspace`] — adapts our salsa workspace to
//!   the trait, serving reads from open editor buffers when available.
//! * [`translate`] — turns `Action`/`ActionPlan` values into LSP
//!   `WorkspaceEdit`s that the editor can apply.
//! * [`triggers`] — pre-computes which detectors a given edit could
//!   possibly affect, so the host can skip detectors whose declared
//!   triggers don't overlap the change.

pub(crate) mod changelog_edits;
pub(crate) mod deb822_edits;
pub(crate) mod format_edits;
pub mod translate;
pub mod triggers;
pub mod workspace;

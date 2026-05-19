//! When and how thoroughly the LSP server should run analysis.
//!
//! Different LSP events justify different amounts of work. Typing in
//! the editor (`did_change`) fires many times per second, so each
//! tick must be cheap. Opening a file (`did_open`) is rare, so a few
//! disk reads or subprocess calls are fine. Explicit user actions
//! (code-action invocation, the CLI `check` command) are the only
//! place we'd hit the network on the user's behalf.
//!
//! [`RunPhase`] captures that distinction in one place so analysis
//! consumers (the lintian-brush detector layer in particular) can map
//! it to whatever cost class they understand.

/// The user-facing event that triggered an analysis run.
///
/// The variants are ordered by how much work each phase budgets, but
/// the mapping to a specific budget is up to each consumer — the LSP
/// host shouldn't bake a particular cost taxonomy into its event
/// dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunPhase {
    /// Triggered by `did_change`. Each keystroke fires this; analysis
    /// must be fast enough to keep up.
    Keystroke,
    /// Triggered by `did_open`. Initial scan of a file the user just
    /// opened; rare enough to allow disk reads and shell-outs.
    Open,
    /// Triggered by an explicit user action (code-action invocation,
    /// "scan now", the CLI `check` command). The user is asking for
    /// the most thorough check we can give them.
    Explicit,
}

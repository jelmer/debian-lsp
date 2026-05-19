//! Lintian-brush-specific phase mapping.
//!
//! Translates the LSP-side [`RunPhase`] (keystroke / open / explicit) to
//! the lintian-brush detector cost ceiling and the network-access flag
//! the detector preferences expect.

use ::lintian_brush::detector::DetectorCost;

pub use crate::phase::RunPhase;

/// Maximum [`DetectorCost`] this phase will run.
pub fn phase_max_cost(phase: RunPhase) -> DetectorCost {
    match phase {
        RunPhase::Keystroke => DetectorCost::Filesystem,
        RunPhase::Open => DetectorCost::Subprocess,
        RunPhase::Explicit => DetectorCost::Network,
    }
}

/// Whether detectors are allowed to use the network in this phase.
pub fn phase_allow_net(phase: RunPhase) -> bool {
    matches!(phase, RunPhase::Explicit)
}

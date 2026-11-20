//! What one call changed in the worktree (#1149): a stub, so its tests
//! compile and fail first.

use std::path::Path;

use crate::formats::regimen::Regimen;

/// The regimen key of the per-call changes lever (#1149).
pub const CALL_CHANGES: &str = "call_changes";

/// Whether each `bash`, `edit` and `write` result says what the call changed
/// (#1149): not read yet.
///
/// # Errors
///
/// None yet.
pub fn call_changes(_regimen: &Regimen) -> Result<bool, String> {
    Ok(false)
}

/// The worktree as the session last counted it (#1149): not counted yet.
#[derive(Debug)]
pub struct Tree;

impl Tree {
    /// A tree over `root`.
    #[must_use]
    pub fn new(_root: &Path) -> Self {
        Self
    }
}

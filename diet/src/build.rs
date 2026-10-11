//! The build these binaries were made from (#757), as `build.rs` read it
//! from `git`: what a served session's log names in `session.start`.

/// The commit and whether its tracked files differed, or `None` where the
/// build had no repository to read.
#[must_use]
pub fn build() -> Option<(String, bool)> {
    let commit = env!("DIET_BUILD_COMMIT");
    let dirty = match env!("DIET_BUILD_DIRTY") {
        "true" => true,
        "false" => false,
        _ => return None,
    };
    (commit.len() == 40).then(|| (commit.to_owned(), dirty))
}

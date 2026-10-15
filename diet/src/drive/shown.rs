//! The roots a scenario's model is shown in place of where it really ran
//! (path-free scenario outputs): `serve --shown-root REAL=SHOWN`, which only
//! a canned substrate takes.
//!
//! A scenario's worktree and recording live in fresh directories, so their
//! paths differ from run to run and machine to machine (`/var/folders/...`
//! here, `/tmp/...` on CI). Wherever a path reaches the model -- a capped
//! output's "saved to", a background shell's `<cwd>` and output file, a
//! glob's absolute listing, the meditation's "saved at" -- it moved the
//! prompt's bytes, its token counts, and the digest of every kept output
//! that printed one. Each real root is rewritten to a fixed placeholder in
//! every string the model is shown and every output kept for it, before the
//! keeping, so the copies and their digests are path-free too. Commands
//! still run in the real directories; the log's metadata (`cwd`, the
//! worktree) stays real, and `session.start`'s `shown_roots` declares the
//! mapping.

use std::collections::BTreeMap;

/// What else a scenario shown its placeholders fixes, as `session.start`'s
/// `scenario_fixed` names it: each background job's id, from its count, and
/// the pid its model is shown, 0.
pub const FIXED: [&str; 2] = ["job_ids", "pids"];

/// The roots, each real spelling with the placeholder shown for it: the
/// root as given and its canonical form (`/tmp/x` is `/private/tmp/x` on
/// macOS, as `getcwd` and a glob report it), longest first, so a root never
/// rewrites part of a longer one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShownRoots {
    spellings: Vec<(String, String)>,
    declared: BTreeMap<String, String>,
}

impl ShownRoots {
    /// No roots: every string passes through unchanged.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Whether there is nothing to rewrite.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.spellings.is_empty()
    }

    /// Add `REAL=SHOWN`: `real` an absolute path, `shown` an absolute
    /// placeholder.
    ///
    /// # Errors
    ///
    /// Why the argument is not one: no `=`, either side relative, or a
    /// placeholder that holds a real root (a rewrite would never settle).
    pub fn add(&mut self, argument: &str) -> Result<(), String> {
        let Some((real, shown)) = argument.split_once('=') else {
            return Err(format!("--shown-root {argument}: REAL=SHOWN"));
        };
        let absolute = |path: &str| path.len() > 1 && std::path::Path::new(path).is_absolute();
        if !absolute(real) || !absolute(shown) {
            return Err(format!(
                "--shown-root {argument}: both sides absolute, neither the root"
            ));
        }
        let real = real.trim_end_matches('/');
        let shown = shown.trim_end_matches('/');
        let mut forms = vec![real.to_owned()];
        if let Ok(canonical) = std::fs::canonicalize(real) {
            let canonical = canonical.to_string_lossy().into_owned();
            if canonical != real {
                forms.push(canonical);
            }
        }
        for form in &forms {
            if shown.contains(form.as_str()) {
                return Err(format!(
                    "--shown-root {argument}: the placeholder holds the real root"
                ));
            }
            self.spellings.push((form.clone(), shown.to_owned()));
        }
        self.spellings
            .sort_by(|(a, _), (b, _)| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        self.declared.insert(shown.to_owned(), real.to_owned());
        Ok(())
    }

    /// `text` with every real root rewritten to its placeholder.
    #[must_use]
    pub fn text(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for (real, shown) in &self.spellings {
            if out.contains(real.as_str()) {
                out = out.replace(real.as_str(), shown);
            }
        }
        out
    }

    /// `bytes` rewritten as [`Self::text`] does, when they are UTF-8; bytes
    /// that are not text pass through.
    #[must_use]
    pub fn bytes(&self, bytes: &[u8]) -> Vec<u8> {
        match std::str::from_utf8(bytes) {
            Ok(text) if !self.is_empty() => self.text(text).into_bytes(),
            _ => bytes.to_vec(),
        }
    }

    /// The mapping as `session.start` declares it: each placeholder, with
    /// the real root it stands for.
    #[must_use]
    pub fn declared(&self) -> Option<BTreeMap<String, String>> {
        (!self.declared.is_empty()).then(|| self.declared.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "diet-shown-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    #[test]
    fn a_root_is_rewritten_as_given_and_as_canonical() {
        let dir = scratch("canonical");
        let given = dir.to_string_lossy().into_owned();
        let canonical = std::fs::canonicalize(&dir)
            .expect("canonical")
            .to_string_lossy()
            .into_owned();
        let mut roots = ShownRoots::none();
        roots
            .add(&format!("{given}=/scenario/worktree"))
            .expect("a root");
        assert_eq!(
            roots.text(&format!("cwd {given}/a and {canonical}/b")),
            "cwd /scenario/worktree/a and /scenario/worktree/b"
        );
        assert_eq!(
            roots.declared(),
            Some(BTreeMap::from([(
                "/scenario/worktree".to_owned(),
                given.clone()
            )]))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_longer_root_is_rewritten_before_a_shorter_one_inside_it() {
        let mut roots = ShownRoots::none();
        roots.add("/t/run=/scenario/recording").expect("a root");
        roots.add("/t/run/wt=/scenario/worktree").expect("a root");
        assert_eq!(
            roots.text("/t/run/wt/x and /t/run/files/y"),
            "/scenario/worktree/x and /scenario/recording/files/y"
        );
    }

    #[test]
    fn an_argument_that_is_not_two_absolute_paths_is_refused() {
        let mut roots = ShownRoots::none();
        assert!(roots.add("/t/run").is_err());
        assert!(roots.add("t/run=/scenario/x").is_err());
        assert!(roots.add("/t/run=scenario").is_err());
        assert!(roots.add("/=/scenario").is_err());
        assert!(roots.add("/t/run=/t/run/shown").is_err());
        assert!(roots.is_empty());
    }

    #[test]
    fn with_no_roots_text_and_bytes_pass_through() {
        let roots = ShownRoots::none();
        assert_eq!(roots.text("/anything"), "/anything");
        assert_eq!(roots.bytes(b"\x89PNG"), b"\x89PNG");
        assert_eq!(roots.declared(), None);
    }
}

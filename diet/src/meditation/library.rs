//! A phase library (#787): the phase documents a session can run in, by name.
//!
//! A library is a directory of phase documents (`diet/formats/phase/`), one
//! per phase, each file named for its phase: the user's own under
//! `$XDG_CONFIG_HOME/discipline/phases/`, or the starter set this crate
//! carries (`diet/phases/starter/`), compiled in. Every transition must name
//! a phase the library has; a library is refused whole, naming the file and
//! the key, rather than read in part.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::formats::phase::{self, Kind, Phase};

/// One phase document, read: the phase and the sha256 of its bytes, which
/// is how the log and the record name the version the session ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// The phase.
    pub phase: Phase,
    /// The sha256 of the document's bytes.
    pub digest: String,
}

impl Document {
    /// How the move to `to` is presented, if this phase lists it.
    #[must_use]
    pub fn kind_of(&self, to: &str) -> Option<Kind> {
        self.phase
            .transitions
            .iter()
            .find(|transition| transition.to == to)
            .map(|transition| transition.kind)
    }
}

/// A library: where it came from, and its documents by phase name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Library {
    /// `starter`, `user`, or the directory it was read from.
    pub source: String,
    /// Each phase's document, by name.
    pub documents: BTreeMap<String, Document>,
}

/// The starter set, by file stem and text.
const STARTER: &[(&str, &str)] = &[
    ("build", include_str!("../../phases/starter/build.toml")),
    ("orient", include_str!("../../phases/starter/orient.toml")),
    ("plan", include_str!("../../phases/starter/plan.toml")),
    ("quick", include_str!("../../phases/starter/quick.toml")),
    ("test", include_str!("../../phases/starter/test.toml")),
];

impl Library {
    /// The starter set this crate carries.
    ///
    /// # Panics
    ///
    /// If the compiled-in set does not read, which this module's tests hold
    /// it always does.
    #[must_use]
    pub fn starter() -> Self {
        Self::of("starter", STARTER.iter().map(|(stem, text)| (*stem, *text)))
            .expect("the starter phase library reads: `library`'s tests hold it")
    }

    /// The library in `dir`: each `*.toml` file a phase document.
    ///
    /// # Errors
    ///
    /// When `dir` cannot be read, holds no document, or a document is
    /// refused (naming the file and the key).
    pub fn read_dir(source: &str, dir: &Path) -> Result<Self, String> {
        let entries = std::fs::read_dir(dir).map_err(|why| format!("{}: {why}", dir.display()))?;
        let mut files: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
            .collect();
        files.sort();
        let mut texts = Vec::new();
        for path in &files {
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or_else(|| format!("{}: no file name", path.display()))?
                .to_owned();
            let text = std::fs::read_to_string(path)
                .map_err(|why| format!("{}: {why}", path.display()))?;
            texts.push((stem, text));
        }
        Self::of(
            source,
            texts
                .iter()
                .map(|(stem, text)| (stem.as_str(), text.as_str())),
        )
    }

    /// A library from documents by file stem.
    ///
    /// # Errors
    ///
    /// No documents; a document that does not read, or whose name is not
    /// its file's; a transition to a phase the library has no document for.
    pub fn of<'a>(
        source: &str,
        texts: impl Iterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self, String> {
        let mut documents = BTreeMap::new();
        for (stem, text) in texts {
            let file = format!("{stem}.toml");
            let phase = phase::parse(text).map_err(|why| format!("{file}: {why}"))?;
            if phase.name != stem {
                return Err(format!(
                    "{file}: phase: `name` is `{}`, and a document is named for its phase",
                    phase.name
                ));
            }
            let digest = crate::digest::sha256_hex(text.as_bytes());
            documents.insert(phase.name.clone(), Document { phase, digest });
        }
        if documents.is_empty() {
            return Err(format!("the phase library `{source}` holds no document"));
        }
        for (name, document) in &documents {
            for (index, transition) in document.phase.transitions.iter().enumerate() {
                if !documents.contains_key(&transition.to) {
                    return Err(format!(
                        "{name}.toml: transitions[{index}]: `to` is `{}`, a phase the library \
                         has no document for",
                        transition.to
                    ));
                }
            }
        }
        Ok(Self {
            source: source.to_owned(),
            documents,
        })
    }

    /// The document of the phase `name`.
    #[must_use]
    pub fn document(&self, name: &str) -> Option<&Document> {
        self.documents.get(name)
    }
}

/// The user's library directory: `$XDG_CONFIG_HOME/discipline/phases`, or
/// `~/.config/discipline/phases` when that is unset.
#[must_use]
pub fn user_dir() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join("discipline").join("phases"))
}

/// The library a regimen's `phase_library` names: `starter`, `user`, or a
/// directory (a leading `~/` is the home directory).
///
/// # Errors
///
/// As [`Library::read_dir`], or no home directory to find the user's in.
pub fn named(word: &str) -> Result<Library, String> {
    match word {
        "starter" => Ok(Library::starter()),
        "user" => {
            let dir = user_dir().ok_or("no home directory to find the user's phase library in")?;
            Library::read_dir("user", &dir)
        }
        path => {
            let dir = match path.strip_prefix("~/") {
                Some(rest) => PathBuf::from(
                    std::env::var_os("HOME").ok_or("no home directory for a `~/` path")?,
                )
                .join(rest),
                None => PathBuf::from(path),
            };
            Library::read_dir(path, &dir)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Library, named};
    use crate::formats::phase::Kind;

    /// #787: the starter set reads -- orient, plan, build, test and quick --
    /// each document named for its file and every move to a phase it has.
    #[test]
    fn the_starter_library_reads_its_five_phases() {
        let starter = Library::starter();
        assert_eq!(
            starter
                .documents
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["build", "orient", "plan", "quick", "test"]
        );
        let orient = starter.document("orient").expect("orient");
        assert_eq!(orient.kind_of("plan"), Some(Kind::Suggested));
        assert_eq!(orient.kind_of("build"), None);
        assert!(
            starter
                .document("quick")
                .expect("quick")
                .phase
                .transitions
                .is_empty()
        );
        assert_eq!(starter.document("plan").expect("plan").digest.len(), 64);
        assert_eq!(named("starter"), Ok(starter));
    }

    /// #787: a library is refused whole, naming the file and the key.
    #[test]
    fn a_library_is_refused_naming_the_file_and_the_key() {
        let plan = "name = \"plan\"\nground_rules = \"x\"\n\n[[transitions]]\nto = \"build\"\nkind = \"suggested\"\n";
        let refused = Library::of("t", [("plan", plan)].into_iter()).expect_err("no build");
        assert!(
            refused.contains("plan.toml: transitions[0]: `to` is `build`"),
            "{refused}"
        );
        let misnamed = Library::of("t", [("draft", plan)].into_iter()).expect_err("misnamed");
        assert!(
            misnamed.contains("draft.toml: phase: `name` is `plan`"),
            "{misnamed}"
        );
        let malformed =
            Library::of("t", [("plan", "name = \"plan\"\n")].into_iter()).expect_err("malformed");
        assert!(
            malformed.contains("plan.toml: phase: no `ground_rules`"),
            "{malformed}"
        );
        assert!(
            Library::of("t", std::iter::empty()).is_err(),
            "no documents"
        );
    }

    /// #787: a directory is a library, its `*.toml` files its documents.
    #[test]
    fn a_directory_is_read_as_a_library() {
        let dir = std::env::temp_dir().join(format!("diet-phases-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory");
        std::fs::write(
            dir.join("quick.toml"),
            "name = \"quick\"\nground_rules = \"x\"\n",
        )
        .expect("written");
        std::fs::write(dir.join("notes.md"), "not a document").expect("written");
        let read = Library::read_dir("mine", &dir).expect("a library");
        assert_eq!(read.documents.len(), 1);
        assert_eq!(read.source, "mine");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

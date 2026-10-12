//! `scripts/resolve-diet.py`'s staleness rule over a tree of its own (#776):
//! the files `diet` is built from are the dep-info's to say, and the dep-info
//! that says is the one about the binary at hand -- rustc's, for the copy in
//! `deps/` cargo took it from, since `cargo test` copies a binary up without
//! writing the `<name>.d` beside it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the repository")
        .to_path_buf()
}

/// A tree shaped like the repository: its manifests, `diet`'s crate root and
/// a grammar it embeds, a test it is not built from, and a binary at
/// `target/debug/diet` -- all of it written an hour ago, the binary half an
/// hour ago. Removed on drop.
struct Tree(PathBuf);

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Tree {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("diet-resolve-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let tree = Self(dir);
        for file in [
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            "diet/Cargo.toml",
            "diet/src/bin/diet.rs",
            "diet/src/lib.rs",
            "diet/formats/grammar.pest",
            "diet/tests/a_test.rs",
        ] {
            tree.write(file, b"x", 60 * 60);
        }
        tree.binary("target/debug/diet", b"the binary");
        tree
    }

    fn path(&self, file: &str) -> PathBuf {
        self.0.join(file)
    }

    /// `file` holding `bytes`, modified `ago` seconds ago.
    fn write(&self, file: &str, bytes: &[u8], ago: u64) {
        let path = self.path(file);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("its directory");
        std::fs::write(&path, bytes).expect("written");
        self.age(file, ago);
    }

    fn age(&self, file: &str, ago: u64) {
        std::fs::File::options()
            .write(true)
            .open(self.path(file))
            .expect("open")
            .set_modified(SystemTime::now() - Duration::from_secs(ago))
            .expect("its time");
    }

    /// An executable `file` holding `bytes`, half an hour old.
    fn binary(&self, file: &str, bytes: &[u8]) {
        use std::os::unix::fs::PermissionsExt as _;
        self.write(file, bytes, 30 * 60);
        std::fs::set_permissions(self.path(file), std::fs::Permissions::from_mode(0o755))
            .expect("executable");
    }

    /// Rustc's dep-info for the artifact at `target/debug/deps/<name>`, in
    /// its own shape: the `.d` as the target, paths relative to the tree,
    /// then each dependency alone.
    fn rustc_depinfo(&self, name: &str, deps: &[&str]) {
        let target = self.path(&format!("target/debug/deps/{name}.d"));
        let mut text = format!("{}: {}\n\n", target.display(), deps.join(" "));
        for dep in deps {
            text.push_str(dep);
            text.push_str(":\n");
        }
        self.write(
            &format!("target/debug/deps/{name}.d"),
            text.as_bytes(),
            30 * 60,
        );
    }

    /// The resolver run from the tree: its exit, and what it said.
    fn resolve(&self) -> (Option<i32>, String) {
        let out = Command::new("python3")
            .arg(root().join("scripts/resolve-diet.py"))
            .current_dir(&self.0)
            .env_remove("DIET_BIN")
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CARGO_BUILD_TARGET_DIR")
            .output()
            .expect("python3 runs the resolver");
        (
            out.status.code(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        )
    }
}

const BUILT_FROM: &[&str] = &[
    "diet/src/bin/diet.rs",
    "diet/src/lib.rs",
    "diet/formats/grammar.pest",
];

#[test]
fn a_test_touched_after_a_cargo_test_build_leaves_diet_fresh() {
    // `cargo test` left `target/debug/diet` -- a copy of `deps/diet-<hash>`
    // -- and no `diet.d`; rustc's `deps/diet-<hash>.d` names what it was
    // built from. A test touched since is not one of them.
    let tree = Tree::new("test-touched");
    tree.binary("target/debug/deps/diet-0123abcd", b"the binary");
    tree.rustc_depinfo("diet-0123abcd", BUILT_FROM);
    // A build of something else, the same size: not the binary's dep-info.
    tree.binary("target/debug/deps/diet-feedf00d", b"other bits");
    tree.rustc_depinfo(
        "diet-feedf00d",
        &["diet/src/bin/diet.rs", "diet/tests/a_test.rs"],
    );
    tree.age("diet/tests/a_test.rs", 0);
    let (code, said) = tree.resolve();
    assert_eq!(code, Some(0), "{said}");
}

#[test]
fn a_file_the_binary_embeds_touched_after_it_still_makes_it_stale() {
    let tree = Tree::new("grammar-touched");
    tree.binary("target/debug/deps/diet-0123abcd", b"the binary");
    tree.rustc_depinfo("diet-0123abcd", BUILT_FROM);
    tree.age("diet/formats/grammar.pest", 0);
    let (code, said) = tree.resolve();
    assert_eq!(code, Some(2), "{said}");
    assert!(said.contains("diet/formats/grammar.pest"), "{said}");
}

#[test]
fn rustcs_dep_info_for_the_binary_beats_a_diet_d_an_older_build_left() {
    // A `diet.d` from an earlier `cargo build`, which `cargo test` did not
    // rewrite when it relinked: it names no grammar. The binary's own
    // dep-info does, and the grammar has changed since.
    let tree = Tree::new("older-diet-d");
    tree.binary("target/debug/deps/diet-0123abcd", b"the binary");
    tree.rustc_depinfo("diet-0123abcd", BUILT_FROM);
    let stale = format!(
        "{}: {}\n",
        tree.path("target/debug/diet").display(),
        tree.path("diet/src/bin/diet.rs").display()
    );
    tree.write("target/debug/diet.d", stale.as_bytes(), 30 * 60);
    tree.age("diet/formats/grammar.pest", 0);
    let (code, said) = tree.resolve();
    assert_eq!(code, Some(2), "{said}");
    assert!(said.contains("diet/formats/grammar.pest"), "{said}");
}

#[test]
fn with_no_dep_info_at_all_the_whole_of_diet_still_counts() {
    // A binary nothing describes -- no copy in `deps/`, no `diet.d` -- is
    // checked against everything under `diet/`, as before.
    let tree = Tree::new("no-dep-info");
    tree.age("diet/tests/a_test.rs", 0);
    let (code, said) = tree.resolve();
    assert_eq!(code, Some(2), "{said}");
    assert!(said.contains("diet/tests/a_test.rs"), "{said}");
}

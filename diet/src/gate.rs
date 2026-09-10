//! The seeded-fault manifests' one reader.
//!
//! Every lane declares its mutations in a `gate.toml` beside its code, and
//! every lane needs the same things checked about that file: that each fault
//! still names source that is there, that it declares the exit it expects,
//! and that the tests it claims to be caught by exist. This module is that
//! check, written once.
//!
//! # It was four copies, and they had drifted
//!
//! The check was written for the `client` lane and copied into `seam`,
//! `isolation` and `drive`. By the time the copies were lifted they did not
//! agree, and the disagreements were not cosmetic:
//!
//! * **`seam` never read `catches` at all.** Its copy stopped after the
//!   anchor: no `expect_exit`, no catchers, no check that the fault was
//!   proved by anything. A seam fault whose only catcher had been renamed or
//!   deleted passed that guard, which is the exact failure the `catches`
//!   check exists to prevent.
//! * **`isolation` matched the catcher's LEAF only.** `client` and `drive`
//!   had already replaced that, with a comment saying why: a name like
//!   `totally::made::up::<leaf>` passes a leaf match, and `cargo test` on
//!   that path runs nothing -- so the fault is applied, no test runs, and the
//!   orchestrator scores it against a name that resolves to nothing.
//!   `isolation` still held the version that comment describes as broken.
//!
//! Neither copy was wrong when it was written. They were wrong by the time
//! anybody looked, which is the two-readers class wearing a `#[cfg(test)]`.
//!
//! # Why it returns a `Result` rather than asserting
//!
//! Because a checker nothing checks is the thing this repository does not
//! ship. [`audit`] takes its file reader as an argument and reports refusals
//! as values, so the tests below can feed it a manifest that is wrong in one
//! named way and prove it says so. Five lanes now depend on this one
//! function; a bug in it would take every one of their gates green and
//! silent.

/// The text between `open` and the first `close` after it.
pub fn between<'a>(haystack: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let after = haystack.find(open)? + open.len();
    let rest = haystack.get(after..)?;
    let end = rest.find(close)?;
    rest.get(..end)
}

/// Check every fault in `manifest` against the lane that declares it.
///
/// `lane` is the lane's whole source, concatenated by the caller -- a catcher
/// has to be findable wherever its test lives, not only in the file the
/// manifest sits beside. `root` is the lane's module name, which is what
/// makes a catcher's module path checkable rather than just its leaf. `read`
/// resolves a fault's `target` to source text.
///
/// Returns the number of faults checked, or the first refusal.
///
/// # Errors
///
/// A fault missing any of `id`, `target`, `anchor`, `expect_exit` or
/// `catches`; an anchor that no longer appears exactly once in its target; an
/// `expect_exit` that is not a number; a catcher whose module path this lane
/// does not contain or whose function is not in the lane's source; a fault
/// with no catchers at all; or a declared `faults` count that is not the
/// number of faults present.
pub fn audit<R>(manifest: &str, lane: &str, root: &str, read: R) -> Result<usize, String>
where
    R: Fn(&str) -> Result<String, String>,
{
    let mut checked = 0_usize;
    for block in manifest.split("\n[[fault]]\n").skip(1) {
        let id = between(block, "id = \"", "\"").ok_or_else(|| "a fault has no id".to_owned())?;
        let target = between(block, "target = \"", "\"")
            .ok_or_else(|| format!("{id}: it names no target"))?;
        let anchor = between(block, "anchor = '''\n", "'''\nbecomes = ")
            .ok_or_else(|| format!("{id}: it carries no anchor"))?;

        // The anchor is the mutation's attachment point. Exactly once: zero
        // means the code moved out from under the fault, and more than once
        // means the mutation would be applied somewhere nobody chose.
        let source = read(target)?;
        let found = source.matches(anchor).count();
        if found != 1 {
            return Err(format!(
                "{id}: its anchor appears {found} times in {target}, not once. The \
                 manifest is stale: either the mutation has to move with the code, \
                 or the fault it seeds is gone."
            ));
        }

        between(block, "expect_exit = ", "\n")
            .ok_or_else(|| format!("{id}: it declares no `expect_exit`"))?
            .parse::<i32>()
            .map_err(|why| format!("{id}: its `expect_exit` is not a number: {why}"))?;

        let catches = between(block, "catches = [\n", "]")
            .ok_or_else(|| format!("{id}: it names no catchers"))?;
        let mut named = 0_usize;
        for line in catches.lines() {
            let Some(name) = between(line, "\"", "\"") else {
                continue;
            };
            // The WHOLE path, not just the leaf. Two shapes are legal. A unit
            // test is `<root>::…::tests::<leaf>` -- rooted in this lane, with
            // `tests` immediately above the leaf -- which admits a nested
            // module's tests without admitting a path the lane does not
            // contain. An integration test has no module path at all, because
            // `cargo test` names it by the function alone.
            let path: Vec<&str> = name.split("::").collect();
            let (Some(leaf), true) = (
                path.last().copied(),
                path.len() == 1
                    || (path.len() >= 3 && path[0] == root && path[path.len() - 2] == "tests"),
            ) else {
                return Err(format!("{id}: `{name}` is not a test this lane contains"));
            };
            let hits = lane.matches(&format!("fn {leaf}(")).count();
            if hits != 1 {
                return Err(format!(
                    "{id}: it claims to be caught by `{name}`, and the lane holds {hits} \
                     tests of that name rather than one. A fault whose catcher was \
                     renamed or deleted is applied, caught by nothing, and scored \
                     against a name."
                ));
            }
            named += 1;
        }
        if named == 0 {
            return Err(format!(
                "{id}: a fault with an empty `catches` is a mutation nothing proves"
            ));
        }
        checked += 1;
    }

    // READ rather than hardcoded. Hardcoded, one lane's copy said 24 beside a
    // doc comment claiming the manifest declared it, and `faults = 9001`
    // passed.
    let declared: usize = between(manifest, "\nfaults = ", "\n")
        .ok_or_else(|| "the package block declares no count".to_owned())?
        .parse()
        .map_err(|why| format!("the declared count is not a number: {why}"))?;
    if checked != declared {
        return Err(format!(
            "the manifest declares {declared} faults and this reader found {checked}"
        ));
    }
    Ok(checked)
}

/// [`audit`] against the repository on disk, panicking with the refusal.
///
/// This is what a lane's drift guard calls. It resolves a `target` relative
/// to the workspace root, the way the orchestrator that applies these
/// mutations does.
///
/// # Panics
///
/// On any refusal [`audit`] reports, and on a manifest that declares no
/// faults at all -- an empty gate is not a gate.
pub fn every_seeded_fault_still_names_source(manifest: &str, lane: &str, root: &str) {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the workspace root");
    let checked = audit(manifest, lane, root, |target| {
        let path = workspace.join(target);
        std::fs::read_to_string(&path)
            .map_err(|why| format!("{} could not be read: {why}", path.display()))
    })
    .unwrap_or_else(|why| panic!("{why}"));
    assert!(
        checked > 0,
        "a manifest that declares no faults is not a gate"
    );
}

#[cfg(test)]
mod tests {

    use super::{audit, between};

    /// A lane source with two tests in it, for the catcher lookups below.
    const LANE: &str = "fn a_catcher_that_exists() {}\nfn another_catcher() {}\n";

    /// One fault, spelled the way a real manifest spells one.
    ///
    /// Every refusal below is this string with exactly one thing changed, so
    /// what each test proves is the difference and not the fixture.
    fn manifest(target: &str, anchor: &str, exit: &str, catches: &str, declared: usize) -> String {
        format!(
            "[package]\nname = \"diet-example\"\nfaults = {declared}\n\n\
             [[fault]]\nid = \"example.a-fault\"\nfailure_class = \"example.a-fault\"\n\
             target = \"{target}\"\nexpect_exit = {exit}\nanchor = '''\n{anchor}'''\n\
             becomes = '''\nsomething else'''\ncatches = [\n{catches}]\n"
        )
    }

    /// The whole fixture, correct, so the refusals below are refusals.
    fn correct() -> String {
        manifest(
            "diet/src/example.rs",
            "the anchor text",
            "101",
            "    \"example::tests::a_catcher_that_exists\",\n",
            1,
        )
    }

    /// A reader that answers with one file's contents, whatever is asked for.
    fn reads(source: &'static str) -> impl Fn(&str) -> Result<String, String> {
        move |_| Ok(source.to_owned())
    }

    #[test]
    fn a_manifest_that_is_true_of_its_lane_is_accepted() {
        assert_eq!(
            audit(&correct(), LANE, "example", reads("the anchor text")),
            Ok(1),
            "the positive control: if this ever fails, every refusal below \
             proves nothing about the thing it names"
        );
    }

    #[test]
    fn between_reads_to_the_first_close_and_answers_nothing_when_there_is_none() {
        assert_eq!(between("id = \"a\" x = \"b\"", "id = \"", "\""), Some("a"));
        assert_eq!(between("no opener here", "id = \"", "\""), None);
        assert_eq!(between("id = \"unterminated", "id = \"", "\""), None);
    }

    #[test]
    fn an_anchor_that_is_no_longer_in_the_source_is_refused() {
        let why = audit(&correct(), LANE, "example", reads("the code moved")).unwrap_err();
        assert!(
            why.contains("appears 0 times") && why.contains("stale"),
            "the refusal says the anchor is gone: {why}"
        );
    }

    #[test]
    fn an_anchor_that_appears_twice_is_refused() {
        let why = audit(
            &correct(),
            LANE,
            "example",
            reads("the anchor text and the anchor text"),
        )
        .unwrap_err();
        assert!(
            why.contains("appears 2 times"),
            "a mutation with two attachment points would be applied somewhere \
             nobody chose: {why}"
        );
    }

    /// The hole the `seam` lane's copy had: no catchers, checked by nothing.
    #[test]
    fn a_fault_that_names_no_catchers_is_refused() {
        let empty = manifest("diet/src/example.rs", "the anchor text", "101", "", 1);
        let why = audit(&empty, LANE, "example", reads("the anchor text")).unwrap_err();
        assert!(
            why.contains("nothing proves"),
            "a mutation no test catches is a mutation nothing proves: {why}"
        );
    }

    /// The hole the `isolation` lane's copy had: a leaf match takes a module
    /// path that resolves to no test, and `cargo test` on it runs nothing.
    #[test]
    fn a_catcher_whose_module_path_this_lane_does_not_contain_is_refused() {
        let invented = manifest(
            "diet/src/example.rs",
            "the anchor text",
            "101",
            "    \"totally::made::up::tests::a_catcher_that_exists\",\n",
            1,
        );
        let why = audit(&invented, LANE, "example", reads("the anchor text")).unwrap_err();
        assert!(
            why.contains("is not a test this lane contains"),
            "the leaf exists in the lane and the path does not, which is \
             exactly what a leaf-only match let through: {why}"
        );
    }

    #[test]
    fn a_catcher_that_was_renamed_out_of_the_lane_is_refused() {
        let renamed = manifest(
            "diet/src/example.rs",
            "the anchor text",
            "101",
            "    \"example::tests::a_catcher_that_was_deleted\",\n",
            1,
        );
        let why = audit(&renamed, LANE, "example", reads("the anchor text")).unwrap_err();
        assert!(
            why.contains("holds 0 tests of that name"),
            "the refusal names the catcher that is gone: {why}"
        );
    }

    #[test]
    fn an_integration_test_named_by_its_function_alone_is_accepted() {
        let integration = manifest(
            "diet/src/example.rs",
            "the anchor text",
            "101",
            "    \"a_catcher_that_exists\",\n",
            1,
        );
        assert_eq!(
            audit(&integration, LANE, "example", reads("the anchor text")),
            Ok(1),
            "`cargo test` names an integration test by the function alone, so \
             a bare leaf is a legal catcher and half the drive lane is one"
        );
    }

    #[test]
    fn an_expect_exit_that_is_not_a_number_is_refused() {
        let unparseable = manifest(
            "diet/src/example.rs",
            "the anchor text",
            "red",
            "    \"example::tests::a_catcher_that_exists\",\n",
            1,
        );
        let why = audit(&unparseable, LANE, "example", reads("the anchor text")).unwrap_err();
        assert!(
            why.contains("`expect_exit` is not a number"),
            "the orchestrator grades on this value: {why}"
        );
    }

    #[test]
    fn a_declared_count_that_is_not_the_number_of_faults_is_refused() {
        let miscounted = manifest(
            "diet/src/example.rs",
            "the anchor text",
            "101",
            "    \"example::tests::a_catcher_that_exists\",\n",
            9001,
        );
        let why = audit(&miscounted, LANE, "example", reads("the anchor text")).unwrap_err();
        assert!(
            why.contains("declares 9001 faults") && why.contains("found 1"),
            "the count is read from the manifest, and this is the case that \
             proves it is not hardcoded: {why}"
        );
    }

    #[test]
    fn a_target_that_cannot_be_read_is_refused_rather_than_skipped() {
        let why = audit(&correct(), LANE, "example", |target| {
            Err(format!("{target} is not there"))
        })
        .unwrap_err();
        assert!(
            why.contains("is not there"),
            "a fault whose target cannot be read has not been checked, and a \
             checker that passes when it could not check is the defect: {why}"
        );
    }
}

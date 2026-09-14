//! `diet-drive` as a program, which is the half a function call cannot reach.
//!
//! `diet::drive::run` has unit tests; **the binary had none**, and a
//! fresh-instance review said so plainly: three hundred and eighty-one lines,
//! zero tests, no lane, and a module docstring claiming the program was what
//! proved a record could be written and read back by a second process.
//! Nothing ran it. Everything unique to it was unexercised -- the regimen-to-
//! regime crossing whose own comment records a defect it once shipped
//! (`temperature = 0.6` reaching the record as the string
//! `Float(Decimal("0.6"))`), the four exit codes, the refusal paths, and the
//! files it writes.
//!
//! **Every test here has `drive` in its name, and that is load-bearing.**
//! `cargo test -- drive` -- which is what this lane's `gate.toml` declares --
//! is a substring filter over test names, and an integration test is named by
//! its function alone with no module path. Two tests here were called
//! `the_regime_crosses_…` and `every_refusal_…`, and the seeded faults they
//! catch were recorded as catching nothing: the filter never reached them.
//!
//! What this file does not do is stand in for the integration lane #23 asks
//! for. That lane runs `diet-drive` and then `diet check-record` from
//! `verify.sh`, which is not this seat's file; this runs both binaries from a
//! test, which is the same evidence in a place this seat can put it.

use std::path::{Path, PathBuf};
use std::process::Command;

const DRIVE: &str = env!("CARGO_BIN_EXE_diet-drive");
const DIET: &str = env!("CARGO_BIN_EXE_diet");

/// The regimen this lane ships, which is the one the lane would run.
fn regimen() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("drive/dev-loop.toml")
}

/// A directory for one test's worktree and output, removed afterwards.
struct Ground {
    base: PathBuf,
}

impl Ground {
    fn make(name: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "diet-drive-cli-{}-{name}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(base.join("tree")).expect("a working tree");
        Self { base }
    }

    fn tree(&self) -> String {
        self.base.join("tree").to_string_lossy().into_owned()
    }

    fn out(&self) -> String {
        self.base
            .join("record.jsonl")
            .to_string_lossy()
            .into_owned()
    }
}

impl Drop for Ground {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn run(program: &str, args: &[&str]) -> (i32, String, String) {
    let output = Command::new(program)
        .args(args)
        .output()
        .unwrap_or_else(|why| panic!("{program} did not run: {why}"));
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn the_drive_writes_a_record_a_second_process_accepts() {
    let ground = Ground::make("round-trip");
    let started = std::time::SystemTime::now();
    let regimen = regimen();
    let (code, out, err) = run(
        DRIVE,
        &[&regimen.to_string_lossy(), &ground.tree(), &ground.out()],
    );
    assert_eq!(code, 0, "stdout={out} stderr={err}");

    // THE WHOLE RESULT LINE, not just `"ok":true`. A review replaced this
    // function's entire body with two fabricated counts -- dropping `turn`,
    // `fork`, `regions`, `captured`, `truncated` and `passed_over`, and
    // reporting a NEGATIVE touch count no run can produce -- and all 593
    // tests stayed green. This JSON is the only surface either entry count
    // ever reaches a human through, since record v0 carries one field.
    //
    // The canned drive is deterministic, so these are the run's real numbers
    // and not a shape check.
    assert!(out.contains("\"ok\":true"), "{out}");
    assert!(out.contains("\"events\":24"), "twenty-four rows: {out}");
    assert!(
        out.contains("\"seams\":1"),
        "one seam, where the script declared it: {out}"
    );
    assert!(
        out.contains("\"audits_unread\":1"),
        "and one audit put and not folded, because the verdict grammar is not \
         built: {out}"
    );

    // Ruling 6's two counts, at the case where they DIVERGE. Turn two's fork
    // repeats turn one's answer word for word, so it touched two entries and
    // created none -- and a reader summing one number to size the object
    // would double it. Asserting only turn one would pass under
    // `entries_created = entries_touched`.
    assert!(
        out.contains("\"entries_created\":2,\"entries_touched\":2,\"fork\":\"fork-1\""),
        "turn one's fork created what it touched: {out}"
    );
    assert!(
        out.contains("\"entries_created\":0,\"entries_touched\":2,\"fork\":\"fork-2\""),
        "and turn two's created NOTHING while touching the same two: {out}"
    );
    // No field carries `-1`, the sentinel `written()` uses for a count that
    // does not fit -- matched as a VALUE, because the temp paths in this same
    // line contain `-1` as a substring and the first version of this
    // assertion failed on its own fixture.
    for ending in [":-1,", ":-1}", ":-1]"] {
        assert!(
            !out.contains(ending),
            "a field carries the unrepresentable-value sentinel: {out}"
        );
    }

    // The verdict comes from the OTHER binary reading the file, which is the
    // whole point: a record this crate can write and cannot read back through
    // its published boundary is not a record.
    let (checked, _, complaint) = run(DIET, &["check-record", &ground.out()]);
    assert_eq!(checked, 0, "check-record refused it: {complaint}");

    // And the digest names a file that exists and hashes to what the summary
    // says. Recomputed here from the bytes on disk rather than from the value
    // the drive carried in memory.
    let product = format!("{}.product", ground.out());
    let bytes = std::fs::read(&product).expect("the product is beside the record");
    let claimed = std::fs::read_to_string(ground.out()).expect("the record");
    let digest = diet::digest::sha256_hex(&bytes);
    assert!(
        claimed.contains(&format!("\"product_sha256\":\"{digest}\"")),
        "the summary's digest is of the product on disk: {digest}"
    );

    // The commands ran in the worktree it was GIVEN, and nowhere else.
    assert!(
        ground.base.join("tree/one.txt").is_file() && ground.base.join("tree/three.txt").is_file(),
        "the script's commands wrote into the working tree"
    );
    // And NOT into whatever directory the test happened to run from. The
    // first version defaulted the worktree to the current directory and
    // dropped these two files into the checkout.
    //
    // Asserted on mtime rather than existence, because a previous run of the
    // seeded fault for this defect leaves them behind -- and a test that a
    // leftover file can hold red is a test that reports the last run instead
    // of this one.
    for stray in ["one.txt", "three.txt"] {
        let Ok(meta) = std::fs::metadata(stray) else {
            continue;
        };
        let age = meta
            .modified()
            .ok()
            .and_then(|at| started.duration_since(at).ok());
        assert!(
            age.is_some(),
            "{stray} was written into the current directory by THIS run"
        );
    }
}

#[test]
fn a_drives_regime_crosses_from_the_regimen_without_being_paraphrased() {
    // The crossing this program shipped a defect in once: a regimen's
    // `temperature = 0.6` reached the record as the string
    // `Float(Decimal("0.6"))`, a regime tag that compares equal to nothing.
    // Only running the program shows it, because the value space either side
    // is what differs.
    let ground = Ground::make("regime");
    let regimen = regimen();
    let (code, _, err) = run(
        DRIVE,
        &[&regimen.to_string_lossy(), &ground.tree(), &ground.out()],
    );
    assert_eq!(code, 0, "{err}");

    let record = std::fs::read_to_string(ground.out()).expect("the record");
    let start = record.lines().next().expect("a start row");
    assert!(
        start.contains("\"temperature\":0.6"),
        "the decimal crosses as a decimal: {start}"
    );
    assert!(
        !start.contains("Decimal(") && !start.contains("Float("),
        "and not as a debug rendering of the reader's own type: {start}"
    );
    assert!(
        start.contains("\"arm\":\"dev-loop\"") && start.contains("\"id\":\"canned\""),
        "and the regime is the regimen's, not one this program wrote down: {start}"
    );

    // The substrate's identity is TYPED and COMPUTED. `weights` is the sha256
    // of the acts the canned server played, taken from the same function the
    // program takes it from -- a digest this test spelled out itself would
    // pass while the program wrote a different one, and being the same on both
    // sides is the entire content of the claim.
    let digest = diet::drive::canned::acts_digest();
    assert!(
        start.contains(&format!("\"acts_sha256\":\"{digest}\"")),
        "the identity is the acts, by digest: {start}"
    );
    // ITS OWN KIND, not `digest`. A canned server runs no weights, and reading
    // its acts digest through the kind that means "these weights" would make
    // that field mean two things told apart only by the engine's name. Gate 1
    // compares a replay exactly and a re-firing within a band, so the kinds
    // have to be distinguishable before either comparison is right. Ruled
    // 2026-09-11.
    assert!(
        start.contains("\"kind\":\"canned\""),
        "and it says which mechanism reproduces it: {start}"
    );
    assert!(
        !start.contains("\"kind\":\"digest\""),
        "not borrowing the kind that means weights: {start}"
    );
}

#[test]
fn every_drive_refusal_has_its_own_exit_code() {
    let ground = Ground::make("refusals");
    let regimen = regimen().to_string_lossy().into_owned();

    // 2: a usage error. Distinct from a drive that could not run only in the
    // text; distinct from 1 and 3 in what it means.
    for args in [
        vec![],
        vec![regimen.as_str()],
        vec![regimen.as_str(), &ground.tree()],
        vec![regimen.as_str(), &ground.tree(), &ground.out(), "a", "b"],
    ] {
        let (code, _, err) = run(DRIVE, &args);
        assert_eq!(code, 2, "args={args:?}");
        assert!(err.contains("usage: diet-drive"), "and the usage: {err}");
    }

    // 1: input that is not usable. Each of these is a different reason and
    // the message has to name it, or a caller cannot tell them apart.
    let thin = ground.base.join("thin.toml");
    std::fs::write(&thin, "arm = \"x\"\n").expect("a regimen missing everything else");
    let cases: Vec<(Vec<String>, &str)> = vec![
        (
            vec!["/nope/absent.toml".to_owned(), ground.tree(), ground.out()],
            "could not be read",
        ),
        (
            vec![
                thin.to_string_lossy().into_owned(),
                ground.tree(),
                ground.out(),
            ],
            "substrate_reasoning",
        ),
        (
            vec![
                regimen.clone(),
                ground
                    .base
                    .join("no-such-tree")
                    .to_string_lossy()
                    .into_owned(),
                ground.out(),
            ],
            "not a directory",
        ),
        (
            vec![
                regimen.clone(),
                ground.tree(),
                ground.out(),
                "https://example.invalid/v1/chat/completions".to_owned(),
            ],
            "not an endpoint",
        ),
    ];
    for (args, says) in cases {
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, out, _) = run(DRIVE, &borrowed);
        assert_eq!(code, 1, "args={args:?} out={out}");
        assert!(out.contains("\"ok\":false"), "a structured refusal: {out}");
        assert!(out.contains(says), "naming {says:?}: {out}");
    }

    // 3: the record could not be filed. Its own code, because a drive that
    // ran and could not write its record is not a drive that could not start
    // -- and this is checked BEFORE anything runs, so the working tree is
    // still empty afterwards.
    let (code, out, _) = run(
        DRIVE,
        &[&regimen, &ground.tree(), "/nope/nowhere/record.jsonl"],
    );
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("nothing has run"), "{out}");
    assert!(
        !ground.base.join("tree/one.txt").exists(),
        "and nothing ran: the output path is claimed before the first call"
    );
}

#[test]
fn a_drive_against_an_endpoint_that_answers_is_still_refused() {
    // THE REFUSAL IS ABOUT IDENTITY, NOT REACHABILITY, and the first version
    // of this test could not tell the two apart. It pointed the program at
    // `127.0.0.1:1`, where nothing listens, so deleting the refusal made the
    // test fail with `the main call did not answer` and exit 2 -- red, for a
    // reason the test had not checked. A review found that by deleting the
    // refusal and watching the test "catch" it through a connection error.
    //
    // So: a real server, on loopback, playing the same acts the canned one
    // plays, which will answer every call this drive makes. Everything works
    // except the one thing that must not.
    //
    // WHAT THE REFUSAL PREVENTS, and why it is worth a server to test: with it
    // gone, this run exits 0 and writes a record whose `weights` is the
    // sha256 of the LOCAL canned acts, while an external endpoint answered
    // every call. That record is schema-legal, `diet check-record` accepts it,
    // and gate 1 reads `Weights::Digest` as reproducible -- so a hosted
    // substrate arrives in the results as a local one that anybody can
    // reproduce. Laundering a substrate's identity is the exact thing the
    // typed-weights ruling exists to refuse.
    let stub = diet::client::stub::Stub::serving(diet::drive::canned::acts())
        .expect("a loopback server to answer the drive");
    let ground = Ground::make("answering-endpoint");
    let (code, out, err) = run(
        DRIVE,
        &[
            &regimen().to_string_lossy(),
            &ground.tree(),
            &ground.out(),
            &stub.url(),
        ],
    );

    assert_eq!(code, 1, "an input this program will not run: {out}{err}");
    assert!(
        out.contains("regimen v1 cannot say"),
        "and it refuses over the substrate's identity, not over reaching the \
         server it just declined to use: {out}"
    );
    // The other half of "not reachability": the stub is still bound and would
    // have answered. A refusal that only happened because nothing was
    // listening would leave this failing.
    assert!(
        !out.contains("did not answer") && !out.contains("could not"),
        "nothing here is a connection failure: {out}"
    );
}

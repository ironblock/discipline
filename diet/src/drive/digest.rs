//! SHA-256, because a record's `product_sha256` is not optional.
//!
//! A summary row requires a digest of the product the session produced, and
//! the schema requires it to be sixty-four lowercase hex characters. A drive
//! that could not compute one could not write a summary, and a record without
//! a summary is not a record. So this is on the critical path rather than a
//! convenience.
//!
//! # The dependency is the ruling, and the reason is worth keeping
//!
//! This module hand-rolled the algorithm once, and it was correct: every
//! FIPS 180-4 vector, the padding boundaries, and a fresh instance's
//! independent 221-input differential against the system's `sha256sum`, all
//! clean. **That was never the argument.** Ruling 7: a cryptographic
//! primitive is the canonical buy, and the digest is the record's IDENTITY
//! primitive -- binary provenance, consumed digests, engine identity -- which
//! makes it the one place a subtle bug is both catastrophic and silent. A
//! hash somebody wrote in an afternoon is a hash somebody has to keep
//! checking, and the cost of being wrong here is every claim ever banked
//! against a digest.
//!
//! # What survives from the hand-rolled version, and why
//!
//! The differential. It now runs against `sha2`'s output rather than this
//! crate's, which turns it from a proof of our own arithmetic into a **cheap
//! conformance check on the dependency** -- the thing that catches a version
//! bump that changes behaviour, a feature flag that swaps an implementation,
//! or a build where the crate is not what the lockfile says. That check costs
//! one `sha256sum` invocation over a directory of staged inputs, and it is the
//! reason the vectors did not simply get deleted with the code they were
//! written for.

use sha2::{Digest as _, Sha256};

/// The SHA-256 of `bytes`, as sixty-four lowercase hex characters.
///
/// The spelling is the one the record requires, so no caller has to remember
/// to lowercase it -- a digest that differs from the same digest by case is
/// the kind of mismatch that gets debugged for an hour.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(64);
    for byte in sha256(bytes) {
        // Written rather than formatted through a width specifier, because a
        // missing zero pad is the classic way a hex encoder produces a digest
        // that is right for most inputs. This half is ours, so it is tested
        // like it is ours.
        out.push(nibble(byte >> 4));
        out.push(nibble(byte & 0x0f));
    }
    out
}

/// One hex digit, lowercase.
fn nibble(value: u8) -> char {
    char::from_digit(u32::from(value), 16).expect("a nibble is below sixteen")
}

/// The SHA-256 of `bytes`, as thirty-two bytes.
#[must_use]
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

#[cfg(test)]
mod tests {

    use super::{sha256, sha256_hex};

    /// The vectors FIPS 180-4 publishes, and the empty message.
    ///
    /// Kept from the hand-rolled version. They no longer prove this module's
    /// arithmetic -- there is none -- but they are what says the dependency
    /// is computing SHA-256 and not something else that also returns 32
    /// bytes.
    #[test]
    fn the_published_vectors_are_what_comes_out() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "FIPS 180-4 B.1"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            "FIPS 180-4 B.2, 56 bytes: the length field lands in a second block"
        );
        assert_eq!(
            sha256_hex(&vec![b'a'; 1_000_000]),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
            "FIPS 180-4 B.3, a million `a`s"
        );
    }

    #[test]
    fn the_spelling_is_the_one_the_record_requires() {
        // The hex encoding is this module's own code, so it is tested as this
        // module's own code. The zero pad is the classic defect: a digest with
        // a byte below 0x10 comes out sixty-three characters long without it.
        let hex = sha256_hex(b"anything");
        assert_eq!(hex.len(), 64, "sixty-four characters, always");
        assert!(
            hex.chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
            "lowercase hex and nothing else: {hex}"
        );
        let padded = sha256(b"5040");
        assert!(
            padded.iter().any(|byte| *byte < 0x10),
            "the fixture has a byte that needs the pad: {padded:?}"
        );
        assert_eq!(sha256_hex(b"5040").len(), 64);
    }

    /// An independent SHA-256, for the differential below to compare against.
    ///
    /// `sha256sum` on a GNU host, `shasum -a 256` on a Mac -- this repository
    /// has seats on both. Both print `<digest>  <path>`, so the caller reads
    /// them the same way.
    ///
    /// Returns the program and the arguments that precede the paths.
    fn an_independent_sha256() -> Option<(&'static str, &'static [&'static str])> {
        for (program, args) in [
            ("sha256sum", &[] as &[&str]),
            ("shasum", &["-a", "256"] as &[&str]),
        ] {
            let answered = std::process::Command::new(program)
                .args(args)
                .arg("--version")
                .output()
                .is_ok_and(|probe| probe.status.success());
            if answered {
                return Some((program, args));
            }
        }
        None
    }

    /// The differential, against the dependency rather than against us.
    ///
    /// A fresh instance ran this by hand while the algorithm was hand-rolled,
    /// as a proof of our arithmetic. Ruling 7 kept it and changed what it is
    /// for: it is now a **conformance check on `sha2`** -- what catches a
    /// version bump that changes behaviour, a feature flag that swaps an
    /// implementation, or a build where the crate is not what the lockfile
    /// says. The published vectors above cannot catch those on their own,
    /// because a wrong implementation that still gets `abc` right is exactly
    /// what a vector set is blind to.
    ///
    /// **The set is built from named groups and its size is whatever they
    /// come to.** The reviewer's run was 221 inputs and this was very nearly
    /// named for that number -- which would have meant padding the set until
    /// it reached 221, fitting the evidence to the label. What each group is
    /// for is written beside it instead.
    ///
    /// # It has no skip, and that is the whole point
    ///
    /// The first version had THREE early returns -- no temp directory, a
    /// staging write that failed, no `sha256sum` -- each returning a
    /// **passing** test. A review closed one of them and left two, under a
    /// comment claiming the class was handled; `TMPDIR` pointed anywhere
    /// unwritable then retired the entire check, took its own seeded fault
    /// green with it, and let a deliberately corrupted digest through. And
    /// the `eprintln!` those paths announced themselves with is swallowed by
    /// libtest on a passing test, so the log could not tell a run that
    /// differentiated from one that did not.
    ///
    /// So there is no skip at all. **This test declares a host requirement:
    /// an independent SHA-256 must be on `PATH`.** A conformance check that
    /// cannot reach its reference implementation has not been performed, and
    /// a green test that did not perform its check is the failure this whole
    /// module is about. Every seat this repository has -- GNU and Mac -- ships
    /// one of the two programs above.
    #[test]
    fn the_dependency_agrees_with_the_system_on_every_shape_that_breaks_a_sha256() {
        let mut inputs: Vec<Vec<u8>> = Vec::new();
        // Every length to 200: spans the one-block and two-block padding
        // cases many times over, and includes 55/56 and 63/64/65 where the
        // length field crosses a block boundary.
        inputs.extend((0..=200_usize).map(|n| vec![b'a'; n]));
        // 119/120/121, which the published vectors miss: the second block's
        // own padding boundary.
        inputs.extend((119..=121_usize).map(|n| vec![b'z'; n]));
        // Every byte value, so a byte-oriented bug that only shows on
        // non-ASCII input cannot hide behind the length cases above.
        inputs.push((0..=255_u8).collect());
        // Embedded NULs, at a block boundary and across one.
        inputs.push(vec![0_u8; 64]);
        inputs.push(vec![0_u8; 65]);
        // High bytes with no ASCII at all, at three lengths.
        inputs.extend([60_usize, 64, 200].map(|n| vec![0xff_u8; n]));
        // A real product, which is what this function is actually called on.
        inputs.push(b"the product a drive produced\nline two\n".to_vec());
        // Bigger than any block count the loops above reach.
        inputs.push(vec![b'q'; 100_003]);

        let (program, prefix) = an_independent_sha256().expect(
            "this test needs an independent SHA-256 on PATH -- `sha256sum` or \
             `shasum` -- to compare `sha2` against. It does not skip when one is \
             absent: a conformance check that cannot reach its reference \
             implementation has not been performed, and a green test that did \
             not perform its check is the defect this module exists to prevent.",
        );

        // Through FILES rather than a pipe. An earlier version fed hex on
        // stdin and decoded it with `xxd`, which is not on every host -- and
        // its absence did not surface as "the tool is missing", it surfaced
        // as a DIGEST MISMATCH on input 1, which is a red that is not about
        // its own fault.
        let ours: Vec<String> = inputs.iter().map(|bytes| sha256_hex(bytes)).collect();
        // Unique per run, and `create_dir` rather than `create_dir_all` so an
        // existing directory is an error rather than something to write into:
        // a predictable name a local process can pre-create, plus writes that
        // follow symlinks, is CWE-377 even in a test.
        let base = std::env::temp_dir().join(format!(
            "diet-digest-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir(&base).expect("a temp directory to stage the inputs in");
        let mut paths = Vec::with_capacity(inputs.len());
        for (at, bytes) in inputs.iter().enumerate() {
            let path = base.join(format!("{at:04}.bin"));
            std::fs::write(&path, bytes).expect("the inputs stage");
            paths.push(path);
        }

        let spawned = std::process::Command::new(program)
            .args(prefix)
            .args(&paths)
            .output();
        let _ = std::fs::remove_dir_all(&base);
        let output = spawned.unwrap_or_else(|why| {
            panic!("`{program}` answered a probe and then could not be run: {why}")
        });
        assert!(
            output.status.success(),
            "`{program}` itself failed, which is not a disagreement about a \
             digest: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let theirs: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .map(str::to_owned)
            .collect();

        assert_eq!(
            theirs.len(),
            inputs.len(),
            "`{program}` answered for every one of the {} inputs, or this \
             compares two lists that are not aligned",
            inputs.len()
        );
        for (at, (ours, theirs)) in ours.iter().zip(&theirs).enumerate() {
            assert_eq!(
                ours,
                theirs,
                "input {at} ({} bytes) disagrees with `{program}`'s digest",
                inputs[at].len()
            );
        }
    }
}

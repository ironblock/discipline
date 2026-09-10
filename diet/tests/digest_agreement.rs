//! Two SHA-256 implementations live in this crate, and this is what stops
//! them disagreeing until one of them is gone.
//!
//! `crate::digest` arrived with the bakeoff runner and `crate::drive::digest`
//! with the drive lane; the merge that brought them into one tree is the merge
//! this file was written on. Neither is wrong -- they agree, including at the
//! padding boundaries where a hand-rolled SHA-256 goes wrong if it goes wrong
//! -- but "they agree today" is a fact nothing was checking, and two
//! implementations of one primitive is exactly the shape this repository
//! refuses for formats.
//!
//! Collapsing them is the runner verb's own work and is sequenced there; doing
//! it inside a merge would be a lane taking another lane's decision. So until
//! then: a digest written by one and checked by the other must be the same
//! digest, and this file fails the moment that stops being true.
//!
//! DELETE THIS FILE when the crate has one digest module. A guard for a
//! duplication that no longer exists is a test that can never fail, which is
//! worse than no test.

/// The lengths a hand-rolled SHA-256 gets wrong: side of, on, and past the
/// 56-byte point where the length field stops fitting in the final block, and
/// exactly one block.
const AWKWARD: &[usize] = &[0, 1, 3, 55, 56, 57, 63, 64, 65, 119, 120, 1000];

#[test]
fn the_two_sha256_implementations_in_this_crate_agree() {
    for &length in AWKWARD {
        // Not all one byte: a padding bug that dropped or repeated a block
        // would be invisible against a run of identical bytes.
        let input: Vec<u8> = (0..length)
            .map(|i| u8::try_from(i % 251).unwrap_or(0))
            .collect();
        assert_eq!(
            diet::digest::sha256(&input),
            diet::drive::digest::sha256_hex(&input),
            "the two implementations disagree on {length} byte(s), so at least one of \
             them is wrong and every digest this crate has written is in question"
        );
    }
}

#[test]
fn and_they_agree_with_the_published_vector() {
    // Both being wrong the same way is the failure the test above cannot see.
    // FIPS 180-4's own "abc".
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    assert_eq!(diet::digest::sha256(b"abc"), ABC);
    assert_eq!(diet::drive::digest::sha256_hex(b"abc"), ABC);
}

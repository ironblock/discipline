//! SHA-256, because a record's `product_sha256` is not optional.
//!
//! A summary row requires a digest of the product the session produced, and
//! the schema requires it to be sixty-four lowercase hex characters. A drive
//! that could not compute one could not write a summary, and a record without
//! a summary is not a record. So this is on the critical path rather than a
//! convenience.
//!
//! **Written here rather than taken as a dependency, and that is a decision
//! to rule on rather than a preference.** This crate has exactly two
//! dependencies and each carries a paragraph saying why it is not a
//! hand-rolled one. The argument for `sha2` is the same argument: a hash
//! somebody wrote in an afternoon is a hash somebody has to audit. The
//! argument against is that a hash is not a format -- there is no second
//! reader to disagree with, only one answer that is right or wrong -- and
//! FIPS 180-4 publishes the vectors that decide it. So the tests below check
//! the published vectors, the two boundaries where a hand-rolled
//! implementation actually breaks (a message that is exactly one block, and
//! one whose length field lands in the padding), and the system's own
//! `sha256sum` on bytes this module hashed. If the maintainer would rather
//! carry `sha2`, this file deletes and the call site does not move.
//!
//! No `unsafe`, which the crate forbids anyway, and no `#[allow]`: the
//! wrapping arithmetic is spelled with the wrapping operators because the
//! algorithm is defined modulo 2^32 and a debug-build overflow panic would be
//! this module failing on correct input.

/// The eight initial hash values: the fractional parts of the square roots of
/// the first eight primes. FIPS 180-4 section 5.3.3.
const INITIAL: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// The sixty-four round constants: the fractional parts of the cube roots of
/// the first sixty-four primes. FIPS 180-4 section 4.2.2.
const K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// The SHA-256 of `bytes`, as sixty-four lowercase hex characters.
///
/// The spelling is the one the record requires, so no caller has to remember
/// to lowercase it -- a digest that differs from the same digest by case is
/// the kind of mismatch that gets debugged for an hour.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = sha256(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        // Written rather than formatted through a width specifier, because a
        // missing zero pad is the classic way a hex encoder produces a digest
        // that is right for most inputs.
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
    let mut state = INITIAL;

    let mut blocks = bytes.chunks_exact(64);
    for block in blocks.by_ref() {
        compress(&mut state, block);
    }

    // The padding: one `0x80`, then zeros, then the message length in BITS as
    // a big-endian u64. It runs to one block when the tail leaves room for
    // the length field and to two when it does not, and the second case is
    // where a hand-rolled implementation is usually wrong -- so it has a test
    // of its own.
    let tail = blocks.remainder();
    let mut last = [0_u8; 128];
    last[..tail.len()].copy_from_slice(tail);
    last[tail.len()] = 0x80;
    let padded = if tail.len() < 56 { 64 } else { 128 };
    let bits = (bytes.len() as u64).wrapping_mul(8);
    last[padded - 8..padded].copy_from_slice(&bits.to_be_bytes());
    for block in last[..padded].chunks_exact(64) {
        compress(&mut state, block);
    }

    let mut out = [0_u8; 32];
    for (slot, word) in out.chunks_exact_mut(4).zip(state) {
        slot.copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// One block, into the state. FIPS 180-4 section 6.2.2.
fn compress(state: &mut [u32; 8], block: &[u8]) {
    let mut w = [0_u32; 64];
    for (slot, bytes) in w[..16].iter_mut().zip(block.chunks_exact(4)) {
        *slot = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }

    // FIPS 180-4 names these `a` through `h`; they are an array here rather
    // than eight bindings because eight single letters is a lint this crate
    // denies, and the index is the letter's position: `v[0]` is `a`, `v[7]`
    // is `h`. The round below is the specification's, in its order.
    let mut v = *state;
    for i in 0..64 {
        let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
        let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
        let temp1 = v[7]
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
        let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
        let temp2 = s0.wrapping_add(maj);

        v.rotate_right(1);
        v[4] = v[4].wrapping_add(temp1);
        v[0] = temp1.wrapping_add(temp2);
    }

    for (slot, value) in state.iter_mut().zip(v) {
        *slot = slot.wrapping_add(value);
    }
}

#[cfg(test)]
mod tests {
    use super::{sha256, sha256_hex};

    /// The vectors FIPS 180-4 publishes, and the empty message.
    ///
    /// These are the whole argument for hand-rolling this: a hash has one
    /// right answer and somebody else published it.
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

    /// The two lengths where a hand-rolled implementation is wrong.
    ///
    /// 55 is the last message that pads inside one block; 56 is the first
    /// that does not, and 64 is exactly one block with a whole block of
    /// padding after it. An implementation that gets `abc` right and these
    /// wrong is the normal way this goes.
    #[test]
    fn the_padding_boundaries_are_the_ones_that_break() {
        // Cross-checked against `sha256sum` when they were written; the
        // constants are what it said.
        assert_eq!(
            sha256_hex(&[b'a'; 55]),
            "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318",
            "55 bytes: the length field still fits"
        );
        assert_eq!(
            sha256_hex(&[b'a'; 56]),
            "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a",
            "56 bytes: it does not, and a second block is padded"
        );
        assert_eq!(
            sha256_hex(&[b'a'; 64]),
            "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb",
            "64 bytes: exactly one block, then a whole block of padding"
        );
        assert_eq!(
            sha256_hex(&[b'a'; 63]),
            "7d3e74a05d7db15bce4ad9ec0658ea98e3f06eeecf16b4c6fff2da457ddc2f34"
        );
    }

    #[test]
    fn the_spelling_is_the_one_the_record_requires() {
        let hex = sha256_hex(b"anything");
        assert_eq!(hex.len(), 64, "sixty-four characters, always");
        assert!(
            hex.chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
            "lowercase hex and nothing else: {hex}"
        );
        // The zero pad, which is the classic hex-encoder defect: a digest
        // with a byte below 0x10 in it comes out 63 characters long without
        // it. `\xa9` is one such input's digest -- found by searching, not
        // assumed.
        let padded = sha256(b"5040");
        assert!(
            padded.iter().any(|byte| *byte < 0x10),
            "the fixture has a byte that needs the pad: {padded:?}"
        );
        assert_eq!(sha256_hex(b"5040").len(), 64);
    }

    /// The system's own tool, on bytes this module hashed.
    ///
    /// The published vectors prove the algorithm; this proves the wiring --
    /// that what a caller passes in is what gets hashed, and that the
    /// spelling agrees with what anybody auditing a record would type.
    #[test]
    fn the_system_agrees_with_it() {
        let subject = "the product a drive produced\nline two\n";
        let ours = sha256_hex(subject.as_bytes());
        let said = std::process::Command::new("sha256sum")
            .arg("-")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write as _;
                child
                    .stdin
                    .take()
                    .expect("a pipe")
                    .write_all(subject.as_bytes())?;
                child.wait_with_output()
            });
        let Ok(output) = said else {
            // A host without `sha256sum` proves nothing here, and the vectors
            // above are what the algorithm rests on. Said out loud rather
            // than skipped silently.
            eprintln!("drive::digest: no `sha256sum` on this host; the vectors stand alone");
            return;
        };
        let theirs = String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned();
        assert_eq!(ours, theirs, "the system's digest of the same bytes");
    }
}

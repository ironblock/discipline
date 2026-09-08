//! SHA-256, because a digest this crate can only check the *shape* of is a
//! digest this crate cannot check.
//!
//! `formats::record` has carried `sha256` fields since the beginning and has
//! only ever validated that they are sixty-four lowercase hex characters.
//! Everything that compared a digest to a file did it somewhere else --
//! `check-results.py` in Python, `verify.sh` in `sha256sum` -- which was fine
//! while nothing in Rust had to. The bakeoff runner has to: it refuses to
//! score a cache whose bytes are not the bytes the record consumed, and a
//! refusal that shells out is a refusal with a second reader.
//!
//! FIPS 180-4, and nothing else: no dependency, because this workspace's only
//! dependency is its parser generator and one function of eighty lines is a
//! smaller thing to own than a supply chain. Held to the standard's own test
//! vectors, including the empty input, which is the case a hand-rolled
//! padding loop gets wrong.

/// The eight initial hash values: the fractional parts of the square roots of
/// the first eight primes.
const H0: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// The round constants: the fractional parts of the cube roots of the first
/// sixty-four primes.
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

/// One 64-byte block into the state.
//
// The working variables are `a` through `h` and the schedule is `w`
// because that is what FIPS 180-4 calls them. A reader checking this
// against the standard reads the standard's names; renaming them for a
// lint would make the one check that matters harder to perform.
#[allow(clippy::many_single_char_names)]
fn block(state: &mut [u32; 8], chunk: &[u8; 64]) {
    let mut w = [0u32; 64];
    for (index, word) in w.iter_mut().enumerate().take(16) {
        let at = index * 4;
        *word = u32::from_be_bytes([chunk[at], chunk[at + 1], chunk[at + 2], chunk[at + 3]]);
    }
    for index in 16..64 {
        let s0 =
            w[index - 15].rotate_right(7) ^ w[index - 15].rotate_right(18) ^ (w[index - 15] >> 3);
        let s1 =
            w[index - 2].rotate_right(17) ^ w[index - 2].rotate_right(19) ^ (w[index - 2] >> 10);
        w[index] = w[index - 16]
            .wrapping_add(s0)
            .wrapping_add(w[index - 7])
            .wrapping_add(s1);
    }

    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for index in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let temp1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[index])
            .wrapping_add(w[index]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }
    for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *slot = slot.wrapping_add(value);
    }
}

/// The SHA-256 of `bytes`, as sixty-four lowercase hex characters.
///
/// The spelling matches what a record's `sha256` field carries and what
/// `sha256sum` prints, so a comparison is a string comparison and there is no
/// second encoding to agree with.
#[must_use]
pub fn sha256(bytes: &[u8]) -> String {
    let mut state = H0;
    let mut chunks = bytes.chunks_exact(64);
    for chunk in &mut chunks {
        let mut fixed = [0u8; 64];
        fixed.copy_from_slice(chunk);
        block(&mut state, &fixed);
    }

    // The tail, the 0x80 terminator, and the length in bits. Two blocks when
    // the remainder leaves no room for the eight length bytes -- the case a
    // 56-byte input reaches and a 55-byte one does not, which is why the
    // standard's 56-byte vector is in the tests below.
    let rest = chunks.remainder();
    let mut tail = [0u8; 128];
    tail[..rest.len()].copy_from_slice(rest);
    tail[rest.len()] = 0x80;
    let blocks = if rest.len() >= 56 { 2 } else { 1 };
    let bits = (bytes.len() as u64).wrapping_mul(8);
    tail[blocks * 64 - 8..blocks * 64].copy_from_slice(&bits.to_be_bytes());
    for index in 0..blocks {
        let mut fixed = [0u8; 64];
        fixed.copy_from_slice(&tail[index * 64..(index + 1) * 64]);
        block(&mut state, &fixed);
    }

    let mut out = String::with_capacity(64);
    for word in state {
        for byte in word.to_be_bytes() {
            out.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
            out.push(char::from_digit(u32::from(byte & 0xf), 16).unwrap_or('0'));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::sha256;

    // FIPS 180-4's own vectors, plus the two lengths a padding loop gets
    // wrong: the empty input, and an input that leaves no room for the length
    // in its own block.
    #[test]
    fn the_standards_vectors() {
        for (input, want) in [
            (
                "",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            (
                "abc",
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
            (
                "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
                "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            ),
            (
                "abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmno\
                 ijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu",
                "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1",
            ),
        ] {
            assert_eq!(sha256(input.as_bytes()), want, "sha256({input:?})");
        }
    }

    // A million 'a's: the standard's long vector, and the only one that
    // exercises the chunking loop more than a handful of times.
    #[test]
    fn the_long_vector() {
        let input = vec![b'a'; 1_000_000];
        assert_eq!(
            sha256(&input),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    // Every length across a block boundary, against the one property a
    // hand-rolled padding can violate: a digest is 64 lowercase hex, and no
    // two adjacent lengths collide.
    #[test]
    fn every_length_around_a_block_boundary_digests() {
        let mut seen = std::collections::BTreeSet::new();
        for length in 0..200 {
            let digest = sha256(&vec![b'x'; length]);
            assert_eq!(digest.len(), 64, "length {length}");
            assert!(
                digest
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
                "length {length}: {digest}"
            );
            assert!(seen.insert(digest), "length {length} collided");
        }
    }
}

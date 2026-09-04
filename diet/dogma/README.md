# `diet/dogma` — the asks, pinned

Every template this program sends to a model, one file per ask under
`templates/`, and `MANIFEST.tsv` pinning each by FNV-1a-64 digest and byte
length. `diet::dogma` (in `diet/src/dogma/`) embeds them at compile time and is
the only way capture code reads them.

**Changing a template is a dogma version bump, not an edit.** Results before
and after are not comparable. The procedure, and `cargo test` refuses until it
is followed:

1. Edit the `.txt`.
2. Bump `diet::dogma::VERSION`.
3. Regenerate `MANIFEST.tsv`: `name`, digest, bytes, tab-separated, one line
   per template. `diet::dogma::digest` is the hash.
4. Say what changed and why in the commit message.

Some templates carry holes, written `{tag}`, that the caller fills through
`Template::fill`. The braces are part of the pinned bytes.

## Provenance

These twenty templates are the research program's prompt set at its freeze on
2026-08-26, byte for byte: the manifest here is that program's manifest, same
names, same digests, same lengths, and the hash function is the same one, so
the two can be diffed line for line. They were extracted there from the harness
source with proven byte identity, and every archived drive that program
recorded was asked in these words. They are `VERSION` 0 here.

The trigger vocabulary and the per-model operating points that sat beside
them in the research program are not here yet; they arrive with the rest of
the dogma port.

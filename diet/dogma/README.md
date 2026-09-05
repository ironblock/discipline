# `diet/dogma` — the asks, pinned

Every template this program sends to a model, one file per ask under
`templates/`, and `MANIFEST.tsv` pinning each by FNV-1a-64 digest and byte
length. `diet::dogma` (in `diet/src/dogma/`) embeds them at compile time and is
the only way capture code reads them.

**Changing a template is a dogma version bump, not an edit.** Results before
and after are not comparable. The procedure:

1. Edit the `.txt`.
2. Regenerate `MANIFEST.tsv`: `name`, digest, bytes, tab-separated, one line
   per template. `diet::dogma::digest` is the hash.
3. Bump `diet::dogma::VERSION` and set `diet::dogma::MANIFEST_DIGEST` to the
   new manifest's digest, in the same commit.
4. Say what changed and why in the commit message.

`cargo test` refuses steps 1 through 3 left half-done: a changed byte without
a manifest line, and a changed manifest without the digest beside the version.
Step 4 is a reviewer's to hold. The files are marked `-text` in
`.gitattributes` so a line-ending conversion cannot change the bytes under the
pin.

Steps 3 and 4 hold for every pin, adopted from review as a repository rule: a
diff that touches `MANIFEST_DIGEST`, `OPERATING_POINTS_DIGEST` or the
vocabulary's pins carries a `VERSION` bump with its reason stated, and a
reviewer reads for it by eye. Nothing mechanical can know a bump is due; the
tests hold only that text and pin agree.

Some templates carry holes, written `{tag}`, that the caller fills through
`Template::fill`. The braces are part of the pinned bytes.

## Provenance

These twenty templates are the research program's prompt set at its freeze on
2026-08-26, byte for byte: the manifest here is that program's manifest, same
names, same digests, same lengths, and the hash function is the same one, so
the two can be diffed line for line. They were extracted there from the harness
source with proven byte identity, and every archived drive that program
recorded was asked in these words. They were `VERSION` 0 here, and are
unchanged under 1, which is where the two pieces below joined them.

Beside the templates, two more pieces of the same dogma, pinned the same way:

- `operating-points.toml`: the per-model operating points (sampler, gate and
  no-think settings per model family), transcribed measurements with their
  receipts. The tables are the research program's byte for byte; the receipts
  are restated as content because they pointed at that program's tickets.
  The text is read through `diet::dogma::operating_points()`, which checks
  `OPERATING_POINTS_DIGEST` before returning it. The constant holding the
  text is private, so the compiler refuses a reader of that constant outside
  the module. A second `include_str!` of the file is a reader too, and no
  compiler refuses that one, so a test in the module refuses any other
  mention of the constant's name or of the file's name under `diet/src` and
  `diet/tests`. A name assembled at run time is beyond that scan, which is
  its stated limit.
- the trigger vocabulary, `diet::dogma::vocabulary`: the surprise words and
  the error words, as Rust constants, pinned by the research program's own
  digest values so the port is checkable from either side.

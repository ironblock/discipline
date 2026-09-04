+++
hypothesis = "Grounded acceptance of extracted facts inverts across seats: a small extractor answering a self-contained ask off the box accepts more, and a different set, than the large model extracting off the warm session prefix."
result = "supported"
product_sha256 = "f99a8c5c92ee619088a06313dc27bf95daeee0ed28af817a7d68d2b4ec48a083"
controls_run = ["seat C, an encoder extracting spans offline from the same sources, as the model-free comparator", "byte-identity of every replayed main-lane request against the drive's own record, asserted per request by the grader's warmth table"]
known_defects = ["The record's regime names one substrate, the drive's; the bakeoff compares three seats on three substrates (the 27B on the box, a 1.7B on the host's CPU, an encoder offline), which the start row cannot carry. The seats' substrates are stated in the hardware field and the claims until the schema has a shape for a comparison across substrates.", "The raw accepted counts are inflated by a fact-parser defect of that era (untagged lines and multi-line quotes counted as facts); the grader reports raw and deduped side by side and lists the two degenerate forks, and the claims cite the deduped figures.", "The judgment axis, whether an accepted fact is true of the mechanism, was not run; only the mechanical axes are graded.", "Seat C's summary names the drive it replayed by a path in the research program's repository, which a reader here cannot open; the drive itself is the human-driven drive of 2026-08-10.", "A recompute over an archive has no session turns; turns and prefill_tokens_total are zero because the schema offers no other shape for a summary whose subject is a recompute."]
turns = 0
prefill_tokens_total = 0

[regime]
arm = "extraction-seat-bakeoff"
substrate = "box-qwen36-27b-ud-q4kxl"
dogma_version = 0
+++

# Extraction acceptance inverts across seats

The second recompute-confirmed row ported from the research program. The
grader over the three seats' replay logs reproduces every one of the 80
report fields, here as it did there.

## Observation

Extraction forks fired off the large model's warm prefix offered many facts
and had almost all of them refused by the grounding gate, and cost the main
slot minutes per session. A cheaper seat was expected to accept fewer.

## Hypothesis

Grounded acceptance of extracted facts inverts across seats: a small
extractor answering a self-contained ask off the box accepts more, and a
different set, than the large model extracting off the warm session prefix.

## Test

One human-driven drive of 2026-08-10 replayed three times on 2026-08-10, its
main lane byte for byte (the grader's warmth table holds each re-sent request
against the drive's own record), with the extraction seat varied: seat A, the
27B on the box off the warm full prefix; seat B, a 1.7B Q4_K_M on the host's
CPU answering the same ask carried in full with no prefix; seat C, an encoder
(gliner_small-v2.1, threshold 0.5, seven labels) over the same sources
offline. Sampler for the model seats: temperature 0.6, top_k 20, top_p 1.0,
max_tokens 4096, thinking off. The encoder's parameters are read from its
summary, which the record consumes by digest; the report does not derive from
them, so they are attested, not reproduced. `grade.py` tallies facts offered against
facts the grounding gate accepted, raw and deduped, and the containment of
one seat's accepted items in another's. `report.json` is the committed
result.

Re-run: `bash recompute.sh` checks every consumed artifact's digest, runs
the grader, and exits 0 when all 80 fields reproduce. A recompute over an
archive has no session turns, so the summary carries zero.

## Results

Seat A: 937 offered, 18 accepted deduped (0.019). Seat B: 1545 offered, 171
accepted deduped (0.111). No accepted item of A is contained in B's; 50 of
B's 171 and 5 of A's 18 are contained in C's 321 spans. The three claims in
`run.jsonl` state each figure; the grader lists the two forks whose raw
counts the parser defect inflated (one in A, one in B).

## Conclusion

Supported, on one drive and one model family. The small seat accepts an
order of magnitude more grounded facts and a disjoint set, so the seats are
not interchangeable and the choice is a regime, not a cost. Unknown: whether
what either seat accepts is true of the mechanism, which the judgment axis
would measure and did not.

Provenance: fired 2026-08-10 in the research program (seats A and B against
its inference box, seat C offline), graded there; re-derived there on
2026-08-24 (80 of 80) and here. The logs are that program's, scrubbed of
home-directory paths inside tool-output strings (2,574 bytes, no graded field
touched: the grader over the scrubbed copies reproduces all 80 fields of the
original report). The grader differs from that program's in eight passages
that named its tickets, one of which is a string the report carries, so 79
of the 80 committed fields are byte-identical to the original and the
eightieth, a "not run" note, differs only in that wording.
The grader's usage note names this directory's own invocation.

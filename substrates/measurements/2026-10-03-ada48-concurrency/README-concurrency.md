# Concurrent-request contamination on the ada48 line, b8-e486f80 vs b7-e7051ef (2026-10-03, inference seat)

Test (`contcheck.py`): four ~10,000-token wikitext-2 test passages (character offsets 0, 60000, 120000, 180000;
newlines collapsed; tokens-per-char measured from the server's tokenizer), each followed by "Continue the passage
above in the same register for several paragraphs.", greedy, 120 tokens, thinking off. Each request is answered
alone (sequentially), then all four at once. A request is contaminated when its concurrent continuation follows
another request's passage. Passage 0 ends in the Ise-class battleship article; 1 is the Battle of Pusan Perimeter;
2 is Hed PE (band); 3 is the ironclad warship article.

- b8-e486f80, MTP on (runs-production-1003.txt) and MTP off (diag-1003.log, arm e486f80-nomtp): concurrent request 1
  continues passage 0 (Ise/Hyūga), request 2 continues passage 1 (Pusan Perimeter, "August 31"); request 0 summarizes
  its own opening (Robert Boulter, Du Fu) instead of continuing. Solo answers are correct.
- b7-e7051ef, MTP on (diag-1003.log arm e7051ef-mtp; runs-production-1003.txt after the revert): all four continue
  their own passages, solo and concurrent.
- Throughput symptom on b8-e486f80 (abwin-llama1-conc.jsonl, the production bench at 10k): 4 concurrent streams with
  MTP, acceptance 0.0, 17.6 tok/s per stream, 64 aggregate (the 2026-09-25 build measured ~0.58 and ~145).
- A code word in the first sentence of each passage was still returned correctly under concurrency at 2k and 10k on
  b8-e486f80: the contamination shows in long continuations, not in a first-sentence lookup.

Servers: the production command line (-np 4 --kv-unified, q8_0 KV, MTP n=3), fresh launch per arm on a side port
for diag-1003.log; the live production server for runs-production-1003.txt. Cause (from source): the cherry-picked
upstream PR #29166 change in set_input_qsa builds blk_entry for one sequence per stream (seq_of_stream) and applies it
to every token; under --kv-unified the four slots share one stream.

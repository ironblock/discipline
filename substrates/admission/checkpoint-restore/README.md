# Checkpoint restore (#143, I4b)

**The question.** When a server reuses a cached prefix, does it generate the same first-token distribution it would generate cold? On a hybrid model the reuse comes from a context checkpoint, and a restore that drops recurrent state diverges here.

This is the constitutional cell the plan specified (D12) and planning answered (Q4): "the tolerance is the larger of cold-against-cold and warm-against-cold on a known-good restore; no-reuse is `unadjudicated`, retried 3 times."

## Procedure

`checkpoint_restore.py measure` sends one-token generation requests to `/completion`, pinned to one slot, with the full distribution unsampled (temperature 1, no top-k, top-p or min-p) and `n_probs` 20:

1. **P + X** primes the slot.
2. **P + Y**, cached: the server reuses P. Its `timings.cache_n` is the number of tokens it reused.
3. **P + Y**, `cache_prompt: false`, three times: cold.

P is 120 fixed lines, and X and Y are two short questions. The prompt's digest is recorded with each measurement.

An attempt whose warm call reused nothing (`cache_n` 0) is retried, up to 3 attempts. The first attempt that reuses is the measurement.

There is no restart. But each call occupies the slot and writes the server's prompt cache (N9), so a window plan on a shared host declares that effect.

## The word

`decide RUNG REFERENCE IDENTITY CRITERION`:

- **Distance:** over the union of each side's top 5 tokens, the largest absolute difference in log-probability. A token missing from the other side's list of 20 makes it infinite.
- **Tolerance:** the largest of three:
  - cold against cold on the rung;
  - cold against cold on the reference;
  - warm against cold on the reference.

  Warm against warm never bounds it, because two warm calls take one cached path (N3).
- **The reference** must pass three checks, or the word is `unadjudicated` with the reason:
  - its GGUF header shows no recurrent keys (`checkpoint_restore.py gguf PATH` reads it);
  - it runs on the same engine binary as the rung, because restore is engine behaviour;
  - its warm continuation (the tokens the warm call reprocessed) is within 10% of the rung's.
- **The word:**
  - `pass`: the rung's warm call reused, and its distance from the cold call is within the tolerance;
  - `fail`: it reused, and the distance is beyond the tolerance;
  - `unadjudicated`, for any of these:
    - the rung reused nothing in 3 attempts;
    - the reference reused nothing;
    - fewer than two cold calls;
    - more attempts than declared;
    - no valid reference.

**Why the continuation lengths must match.** Measured on 2026-09-29 on the dense 1.7B, on the floor's engine binary, CPU-only:
- **Deterministic cold path:** cold against cold is exactly 0, at 8 threads and at 4, and across the two.
- **Warm depends on shape:** warm against cold reads 0.80 when the warm call reprocesses 6 tokens, and 0.31 when it reprocesses 90.
- **Warm depends on history:** two warm calls with the same 6-token continuation differ from each other by 0.56, depending on which request computed the cached prefix.

So a known-good restore moves the distribution by an amount set by the batch composition that computed the cache. A tolerance taken at one length does not bound another. The rung's reuse stops at a checkpoint, so its warm call can reprocess hundreds of tokens. The reference is therefore primed with P short of N lines (`--prime-drops-lines N`), which makes its warm call reprocess about as many.

## Tests

- **`python3 checkpoint_restore.py selftest`** covers:
  - **against a scripted server:** pass, divergent, no reuse on every attempt, and no reuse then reuse on the retry. For each, the test checks the word, the number of attempts, one token per request pinned to the slot, and every cold call being P + Y uncached;
  - **the reference's shortened prime;**
  - **11 `decide` cases:**
    - a recurrent reference, a reference on another binary, an unread header;
    - a reference with no reuse, a reference of another continuation length;
    - one cold call, too many attempts;
    - a missing top token;
    - the rung's own cold floor widening the tolerance;
    - exactly at the tolerance, and just over it;
  - **2 distance checks.**
- **`python3 mutants.py`** seeds 12 mutations. The selftest must exit 1 on each, and **all 12 are killed**:
  - the comparator ignoring the log-probabilities (the plan's first named fault);
  - the `cache_n` check removed (the plan's second);
  - no retry after an attempt with no reuse;
  - a missing token made harmless;
  - a recurrent reference accepted;
  - a reference on another engine accepted;
  - the shape match removed;
  - the reference's warm-against-cold, or the rung's cold floor, left out of the tolerance;
  - the boundary moved;
  - the reference primed with all of P;
  - the attempt count unenforced.

Wiring the selftest into `verify.sh`, and the faults into `faults.toml`, is track one's, as the plan assigns it.

## Use

```
python3 checkpoint_restore.py gguf MODEL.gguf > header.json            # on the box: rung and reference headers
python3 checkpoint_restore.py measure --endpoint http://HOST:PORT --role rung --out rung.json
python3 checkpoint_restore.py measure --endpoint http://HOST:REFPORT --role reference --prime-drops-lines N --out reference.json
python3 checkpoint_restore.py decide rung.json reference.json identity.json criterion.toml
```

`identity.json` holds:
- `rung_engine` and `reference_engine`: the sha256 of each server's running exe, read from `/proc/<pid>/exe`;
- `reference_header`: the reference's `gguf` output.

Choose N so that the reference's warm `prompt_n` lands within 10% of the rung's. At P's 120 lines of about 22 tokens each, N is roughly the rung's warm `prompt_n` divided by 22.

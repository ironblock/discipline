# Checkpoint restore (#143, I4b)

**The question.** When a server reuses a cached prefix, does it generate the same first-token distribution it would generate cold? On a hybrid model the reuse comes from a context checkpoint, and a restore that drops recurrent state diverges here.

This is the constitutional cell the plan specified (D12) and planning answered (Q4): "the tolerance is the larger of cold-against-cold and warm-against-cold on a known-good restore; no-reuse is `unadjudicated`, retried 3 times."

## Procedure

`checkpoint_restore.py measure` sends one-token generation requests to `/completion`, pinned to one slot, with the full distribution unsampled (temperature 1, no top-k, top-p or min-p) and `n_probs` 20:

1. **P + X** primes the slot.
2. **P + Y**, cached: the server reuses P. Its `timings.cache_n` is the number of tokens it reused.
3. **P + Y**, `cache_prompt: false`, three times: cold.

P is 120 fixed lines, and X and Y are two short questions. The prompt's digest is recorded with each measurement.

An attempt whose warm call reused nothing (`cache_n` 0) is retried, up to 3 attempts. The first attempt that reuses is the measurement, and it draws warm three times, re-priming before each draw, because the warm path depends on which request computed the cache (measured, below).

There is no restart. But each call occupies the slot and writes the server's prompt cache (N9), so a window plan on a shared host declares that effect.

## The word

`decide RUNG REFERENCE IDENTITY CRITERION`:

- **Distance:** over the union of each side's top 5 tokens, the largest absolute difference in log-probability. A token missing from the other side's list of 20 makes it infinite.
- **Tolerance:** the largest of three:
  - cold against cold on the rung;
  - cold against cold on the reference;
  - warm against cold on the reference, the largest of its three warm draws.

  Warm against warm never bounds it, because two warm calls take one cached path (N3).
- **The reference** must pass these checks, or the word is `unadjudicated` with the reason:
  - its GGUF header shows no recurrent keys: SSM, recurrent, `full_attention_interval`, RWKV, short-conv or Mamba (`checkpoint_restore.py gguf PATH` reads it);
  - it runs on the same engine binary as the rung, because restore is engine behaviour, with both binaries' digests present;
  - it is of one procedure with the rung: the same prompt digest and sampler, an `n_probs` list at least twice the compared top-k, and the roles as declared;
  - its warm continuation (the tokens the warm call reprocessed) is within 10% of the rung's.
- **The word:**
  - `pass`: the rung's warm calls reused, and every one of its three draws is within the tolerance of the cold call;
  - `fail`: they reused, and some draw is beyond the tolerance;
  - `unadjudicated`, for any of these:
    - the rung reused nothing in all 3 declared attempts (fewer attempts decide nothing);
    - a repeated warm draw reused nothing;
    - the tolerance is infinite (a compared token missing from a bounding call's list): an infinite bound is never a pass;
    - the reference reused nothing;
    - fewer than two cold calls;
    - more attempts than declared;
    - no valid reference.

**Why the continuation lengths must match.** Measured on 2026-09-29 on the dense 1.7B, on the floor's engine binary, CPU-only:
- **Deterministic cold path:** cold against cold is exactly 0, at 8 threads and at 4, and across the two.
- **Warm depends on shape:** warm against cold reads 0.80 when the warm call reprocesses 6 tokens, and 0.31 when it reprocesses 90.
- **Warm depends on history:** two warm calls with the same 6-token continuation differ from each other by 0.56, depending on which request computed the cached prefix.

So a known-good restore moves the distribution by an amount set by the batch composition that computed the cache. A tolerance taken at one length does not bound another, and one warm draw does not bound the history: hence the matched length, and three warm draws on each side. The reference is primed with a shortened P rather than the rung's P + X, so its cache history differs from the rung's; the three draws bound that as far as three draws can, and it is disclosed. These measurements were taken by a one-off script; their committed record is the floor's cell, which runs this instrument on the same reference at two lengths. The rung's reuse stops at a checkpoint, so its warm call can reprocess hundreds of tokens. The reference is therefore primed with P short of N lines (`--prime-drops-lines N`), which makes its warm call reprocess about as many.

## Tests

- **`python3 checkpoint_restore.py selftest`** covers:
  - **against a scripted server:** pass, divergent, no reuse on every attempt, no reuse then reuse on the retry, reuse only on the last attempt, and a later warm draw diverging. For each, the test checks the word, the number of attempts, one token per request pinned to the slot, and every cold call being P + Y uncached;
  - **the reference's shortened prime;**
  - **24 `decide` cases:**
    - references that are not references: recurrent keys, `full_attention_interval` alone, RWKV, another binary, missing digests, an unread header, no reuse, another continuation length;
    - the 10% shape boundary, on each side;
    - one cold call, too many attempts, too few attempts with no reuse, a repeated warm draw without reuse;
    - a missing top token (fail), an infinite tolerance (never a pass);
    - the rung's cold floor over every pair, the reference's largest warm draw, any rung draw over the tolerance;
    - exactly at the tolerance, and just over it;
    - measurements of different prompts, and an `n_probs` list too shallow;
  - **2 distance checks.**
- **`python3 mutants.py`** seeds 23 mutations. The selftest must exit 1 on each, and **all 23 are killed**. Among them:
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
  - the attempt count unenforced, or one short;
  - an infinite tolerance allowed; missing engine digests accepted; the one-procedure check removed;
  - only the first warm draw used, on either side; no repeated warm draws;
  - too few attempts accepted; a repeated draw without reuse accepted;
  - the cold floor over the first pair only; `full_attention_interval` not read as recurrent.

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

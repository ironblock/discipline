# Checkpoint restore (#143, I4b)

**The question.** When a server reuses a cached prefix, does it generate the same first-token distribution it would generate cold? On a hybrid model the reuse comes from a context checkpoint, and a restore that drops recurrent state diverges here.

This is the constitutional cell the plan specified (D12) and planning answered (Q4): "the tolerance is the larger of cold-against-cold and warm-against-cold on a known-good restore; no-reuse is `unadjudicated`, retried 3 times."

## Procedure

`checkpoint_restore.py measure` sends one-token generation requests to `/completion`, pinned to one slot, with the full distribution unsampled (temperature 1, no top-k, top-p or min-p) and `n_probs` 20:

1. **P + X** primes the slot.
2. **P + Y**, cached: the server reuses P. Its `timings.cache_n` is the number of tokens it reused.
3. **P + Y**, `cache_prompt: false`, three times: cold.

P is 120 fixed lines, and X and Y are two short questions. The prompt's digest is recorded with each measurement.

An attempt whose warm call reused nothing (`cache_n` 0) is retried, to 3 attempts in all: the first plus two retries, which is how this instrument reads Q4's "retried 3 times". The first attempt that reuses is the measurement. On the rung it is one warm call against cold, as ruled. On the reference it draws warm three times, re-priming before each draw, because the warm path depends on which request computed the cache (measured, below). If a later draw reuses nothing, the whole attempt counts as no reuse and is retried.

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
  - every one of its warm continuations (the tokens each warm draw reprocessed) is within 10% of the rung's, since each draw bounds.
- **The word:**
  - `pass`: the rung's warm call reused, and its distance from the cold call is within the tolerance;
  - `fail`: it reused, and the distance is beyond the tolerance;
  - `unadjudicated`, for any of these:
    - the rung reused nothing in all 3 declared attempts (fewer attempts decide nothing);
    - the reference drew warm fewer times than declared (`reference_warm_draws`, 3);
    - the tolerance is infinite (a compared token missing from a bounding call's list): an infinite bound is never a pass;
    - the reference reused nothing;
    - fewer than two cold calls;
    - more attempts than declared;
    - no valid reference.

**Why the continuation lengths must match.** Measured on 2026-09-29 on the dense 1.7B, on the floor's engine binary, CPU-only:
- **Deterministic cold path:** cold against cold is exactly 0, at 8 threads and at 4, and across the two.
- **Warm depends on shape:** warm against cold reads 0.80 when the warm call reprocesses 6 tokens, and 0.31 when it reprocesses 90.
- **Warm depends on history:** two warm calls with the same 6-token continuation differ from each other by 0.56, depending on which request computed the cached prefix.

So a known-good restore moves the distribution by an amount set by the batch composition that computed the cache. A tolerance taken at one length does not bound another, and one warm draw does not bound the history: hence the matched length, and the reference's three warm draws, whose largest distance bounds. More reference draws can only widen the tolerance; the rung stays at the one draw the ruling names, so a healthy rung is not made likelier to fail. What the three draws do not bound: they follow one request sequence, so on a reproducible path they may repeat each other, and none varies the reference's shortened prime against the rung's full P + X. That difference in cache history is disclosed, not bounded. Each re-prime's reply is kept (`prime_more`), so what it reused is on the record.

**For planning, on the pass condition.** A pass needs only `cache_n` > 0, as D12 says, so a warm call that reused almost nothing would pass without exercising a checkpoint restore. `cache_n` and the warm continuation's length are on the record for that reason; a floor on reuse would be a ruling.

**For planning to ratify.** Two parts of the tolerance go beyond D12's literal text, and both can only widen it (a divergence likelier to pass, never a healthy restore likelier to fail): the reference's own cold-against-cold, and the largest of the reference's three warm draws instead of one. Both follow from the measurements above; the ruling named one known-good warm-against-cold draw. These measurements were taken by a one-off script; their committed record is the floor's cell, which runs this instrument on the same reference at two lengths. The rung's reuse stops at a checkpoint, so its warm call can reprocess hundreds of tokens. The reference is therefore primed with P short of N lines (`--prime-drops-lines N`), which makes its warm call reprocess about as many.

## Tests

- **`python3 checkpoint_restore.py selftest`** covers:
  - **against a scripted server:** pass, divergent, no reuse on every attempt, no reuse then reuse on the retry, reuse only on the last attempt, and a later warm draw evicted (the attempt retried). For each, the test checks the word, the number of attempts, one token per request pinned to the slot, and every cold call being P + Y uncached;
  - **the reference's shortened prime**, with every warm and cold call still P + Y;
  - **the GGUF reader** on a synthetic header, finding the architecture and a recurrent key past twenty others;
  - **36 `decide` cases:**
    - references that are not references: recurrent keys, `full_attention_interval` alone, RWKV, another binary, missing digests, an unread header, no reuse, another continuation length;
    - the 10% shape boundary, on each side;
    - one cold call, too many attempts, too few attempts with no reuse, a repeated warm draw without reuse;
    - a missing top token (fail), an infinite tolerance (never a pass);
    - the rung's cold floor over every pair, the reference's largest warm draw, a reference with too few draws, the rung judged on its one warm call;
    - a cold top token missing from the warm list (the union of both sides), tokens below the top 5 not compared;
    - exactly at the tolerance, and just over it;
    - measurements of different prompts or samplers, or of a prompt or sampler not the instrument's own; roles not as declared; an `n_probs` list too shallow; a truncated engine digest;
    - a later reference draw of another length; a reference with one cold call; a recurrent architecture name alone;
    - the reference's own cold floor as the largest part;
  - **2 distance checks.**
- **`python3 mutants.py`** seeds 39 mutations, in the script and in `criterion.toml`. The selftest must exit 1 on each, and **all 39 are killed**. Among them:
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
  - only the reference's first warm draw used; the reference's draw count unenforced; no repeated warm draws; the rung judged on its largest draw;
  - too few attempts accepted; an eviction on a later draw not retried; a reusing attempt judged by its first draw alone;
  - the comparator over one side's top-k only; the sampler or roles comparison dropped; the engine digest matched by prefix; six tokens compared;
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

# Pre-registration: the parity fire on a TabbyAPI line, identity routing (#393 ruling 3)

Written before any fork of the fire runs, as the maintainer's ruling 3 on #393 (5986337117) requires: the parity fire's routing
check, which #115's and the candidate's fires read from `timings.draft_n`, is replaced for a line whose engine reports no
`draft_n`. Everything else in the fire is the candidate's pre-registration (#143, 5894139110 and 5921525110): the same
instrument (`extraction-acceptance-inverts`), the same arms, the same counted-fork floor, the same band method, the same
word rule. Only the routing check changes.

## What replaces the check

Seat A (the line under admission) and seat B's interview forks go to the candidate endpoint; seat B's extraction forks go
to the CPU-side server and keep their negative check (no `draft_n`, no reasoning, no think block).

1. **Instance identity.** `tabby_identity.py` runs on the host before the first fork and after the last, and `--expect` the
   registry entry's components; `box.json`'s `routing.instance` is the registry instance id read at both ends.
2. **`/v1/model`'s id at every request.** One `tabby_route_proxy.py` per seat fronts the endpoint. Before it forwards each
   POST it reads `GET /v1/model` and records the id. `box.json` carries `routing = {model_id, instance, checks: {A: [...], B: [...]}}`.
3. **`model_id`** is read from `/v1/model` once, before the window's first fork, and committed in the fire's config before that fork.
4. **`apply.py` with `routing = "identity"`**, in the unadjudicated checks, asks:
   - `box.json` carries the `routing` object, its `model_id` equals the config's, and its `instance` equals the box's instance before and after;
   - for each seat, at least as many checks as candidate-routed fork responses (seat A: all; seat B: its interview responses), every one equal to `model_id`;
   - the `draft_n` requirement on the candidate's responses is not asked; seat B's extraction check is unchanged.

## What voids the fire

Any failing check above makes the word `unadjudicated`, every reason listed, as for any other unadjudicated check. **A fire
whose routing check cannot run is void:** a missing or malformed `routing` object, a proxy that died mid-fire (fewer checks
than requests), a `/v1/model` read that failed (recorded `null`, read as another id), an instance that changed across the fire.
A void fire is re-fired after the window, not read.

## What is not changed

The band, its method, level, resamples and seed; the word rule; the seat-A floor; the counted-fork floor of 25 of 31;
the plan check; the disclosure. A fire under `routing = "draft_n"` (every earlier fire) reproduces its recorded verdict
byte for byte (`parity/selftest.sh`), and `apply.py --selftest` carries seven new fixtures for the identity routing.

## Known limit, stated now

The check shows the endpoint that answered is the instance and model the registry names, at every request. It does not show a
token-level property of the answer, and nothing in this fire's word rests on one (output invariance is `n/a`, ruling 2).

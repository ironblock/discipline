# PRINCIPLES
- Truth over validation: It doesn't matter _who_ is right, it matters _what_ is right.
- Attribution over summary: A pointer to a line in a file outlives today's description of it.
- Trust, but verify: Assertions are testable, documentation makes testing easy.
- Reject a faulty premise: Is X the best or only way to achieve Y?
- Don't invent identifiers: prefer semantic descriptions over invented IDs, prefer IDs to come from a registry.
- Code and design: change as much as neccessary, and as little as possible.
- Be concise.


# TICKETS
- Edit the description, don't comment: Tools, agents, and people may only look at the body.
- Edits refine, changes cancel: An edit that's a materially different task is a new ticket.
- One work item per issue: An issue that describes several is closed in favor of successors that each hold one. You can close one of three related tickets; you can't close a third of one ticket.
- A ticket closes with a reason, and the reason is a label: `close: refiled` (replaced by tickets that each ask one thing), `close: out-of-scope` (outside the code's declared scope), `close: mechanism-off` (hardens machinery that is turned off), `close: no-defect` (no observable defect today, by its own body), `close: lives-elsewhere` (an issue used as documentation, whose content now lives in a file in the tree). The closing edit adds a dated line pointing at the record of the decision.
- Prototype first: Nothing is built before the prototype has every theorized lever working unless the prototype needs it (the initiatives, `docs/releases.md`). Everything else goes to a `Later —` milestone that names the condition on which it returns (the grooming of 2026-10-06, #495).
- Test scheduling and device status never go through GitHub: Coordinate windows and a machine's state directly, in private files and notes local to the device under test or to the system monitoring the test. A results directory records what ran, after the fact; it is not where a run is arranged.
- Done means done. A partial result is not a negative result. A flaky gate trains every reader to scroll past it.


# README.MD / AGENTS.MD
- Progressive disclosure: don't frontload everything in a root directory, provide the information in the directory where it becomes relevant.
- Rules live where they are read: A rule in a comment governs whoever read the comment. Put it in the operative file, in the directory where it applies.


# DOCUMENTATION
- GitHub issues are not documentation: A ruling, a rule or a fact the program depends on lives in a file in this repository, in the directory where it is read. An issue may point at that line; the line never depends on the issue. The program's state is in `docs/`, and `docs/AGENTS.md` says who may change which line.
- Written from the record, approved by the author: Documentation is written fresh from the record (the code, results, the maintainer's own words), never extracted as a chore from threads; extraction during writing loses almost nothing, extraction as a chore has a measured fabrication rate. Agents write it. A line that states the program's intent is a significant decision and needs the maintainer's approval, in any form (`docs/AGENTS.md`).
- Undeclared intent is a vacuum: Declare the intent, or expect confident re-derivation from whatever threads are lying around. *Specimen: a single paraphrased line in a handoff became a whole binary, a README-edit recommendation, and an argument for both.*
- If the brief doesn't settle it, stop and ask: Never fill a gap silently. An agent at full momentum that stops at the judgment boundary and asks for the line is doing the most valuable thing it can do.


# TESTING
- A test that cannot fail is not a test: Write RED tests, then make them pass.
- If you're asserting that a system works a certain way, write a test that fails if it doesn't.
- Two numbers, not one: tests that should pass, pass; tests that should fail, fail. The two numbers are independently useful signals, so report both. *Specimen: "all 159 behave as expected" hid that 101 of them were wrong answers graded wrong.*
- A verdict through `grep` is not a gate. `cargo test | tee` returns tee's status, not cargo's; a `&&` following a `grep` passed on nothing.
- An injection's verdict reads the tree delta AND the exit status. A mutation that exits 1 after changing the tree is BROKEN, not a green accusation against a working check. *Specimen: an injection that edited a record and then died reported `GREEN <-- THE GATE DID NOT FIRE` against `check-results.py`, which was working; both readers of "did the injection take" had discarded `$?`.*
- Any fixture text that exists twice has one source. Two copies of the same four lines is one fix and one survivor, and the survivor is found by a seventeen-minute selftest rather than by review.

# MEASUREMENT
- A measurement that cannot come out positive is not a measurement: the mirror of a test that cannot fail. *Specimen: an audit of 27 small-model verdicts found 14 instrument-limited and 11 partly so.*
- Controls first, on the real interface: before scoring, show the instrument catching a known positive and staying quiet on a known null, through the exact path it will run on (model, endpoint, prompt, output mode, labels).
- The ceiling comes before the bar: compute the ceiling and the floor from the labels before choosing a bar. A bar above the ceiling is a verdict written in advance.
- Declare the smallest effect it can see, from N and label agreement. If that is larger than any plausible effect, the result is instrument-limited before it runs.
- Check the input can carry the signal: a second reader given only what the detector sees sets the detector's real ceiling.
- Three verdicts: measured; not like this, not today (a measured negative); instrument-limited. A null from an instrument that failed any rule above is instrument-limited, whatever the number says.
- Review before the run, not after: a separate reviewer gets the plan and one question, "could this come out positive, and what result would show the instrument broken?"

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
- A ticket closes with a reason, and the reason is a label: `close: refiled` (replaced by tickets that each ask one thing), `close: out-of-scope` (outside the code's declared scope), `close: mechanism-off` (hardens machinery that is turned off), `close: no-defect` (no observable defect today, by its own body). The closing edit adds a dated line pointing at the record of the decision.
- Feature complete first: Nothing is built before the product is feature complete unless the drive and `exercise` loop needs it. Everything else goes to a `Later —` milestone that names the condition on which it returns (the grooming of 2026-10-06, #495).
- Test scheduling and device status never go through GitHub: Coordinate windows and a machine's state directly, in private files and notes local to the device under test or to the system monitoring the test. A results directory records what ran, after the fact; it is not where a run is arranged.
- Done means done. A partial result is not a negative result. A flaky gate trains every reader to scroll past it.


# README.MD / AGENTS.MD
- Progressive disclosure: don't frontload everything in a root directory, provide the information in the directory where it becomes relevant.
- Rules live where they are read: A rule in a comment governs whoever read the comment. Put it in the operative file, in the directory where it applies.


# DOCUMENTATION
- GitHub issues are not documentation: A ruling, a rule or a fact the program depends on lives in a file in this repository, in the directory where it is read. An issue may point at that line; the line never depends on the issue.
- Authored, never inherited: Documentation is written fresh from the record by the person who holds the intent. Extraction during authorship loses almost nothing; extraction as a chore has a measured fabrication rate. Agents compile; the author writes.
- Undeclared intent is a vacuum: Declare the intent, or expect confident re-derivation from whatever threads are lying around. *Specimen: a single paraphrased line in a handoff became a whole binary, a README-edit recommendation, and an argument for both.*
- If the brief doesn't settle it, stop and ask: Never fill a gap silently. An agent at full momentum that stops at the judgment boundary and asks for the line is doing the most valuable thing it can do.


# TESTING
- A test that cannot fail is not a test: Write RED tests, then make them pass.
- If you're asserting that a system works a certain way, write a test that fails if it doesn't.
- A verdict through `grep` is not a gate. `cargo test | tee` returns tee's status, not cargo's; a `&&` following a `grep` passed on nothing.
- An injection's verdict reads the tree delta AND the exit status. A mutation that exits 1 after changing the tree is BROKEN, not a green accusation against a working check. *Specimen: an injection that edited a record and then died reported `GREEN <-- THE GATE DID NOT FIRE` against `check-results.py`, which was working; both readers of "did the injection take" had discarded `$?`.*
- Any fixture text that exists twice has one source. Two copies of the same four lines is one fix and one survivor, and the survivor is found by a seventeen-minute selftest rather than by review.

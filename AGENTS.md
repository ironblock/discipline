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
- Done means done. A partial result is not a negative result. A flaky gate trains every reader to scroll past it.


# README.MD / AGENTS.MD
- Progressive disclosure: don't frontload everything in a root directory, provide the information in the directory where it becomes relevant.
- Rules live where they are read: A rule in a comment governs whoever read the comment. Put it in the operative file, in the directory where it applies.


# DOCUMENTATION
- Authored, never inherited: Documentation is written fresh from the record by the person who holds the intent. Extraction during authorship loses almost nothing; extraction as a chore has a measured fabrication rate. Agents compile; the author writes.
- Undeclared intent is a vacuum: Declare the intent, or expect confident re-derivation from whatever threads are lying around. *Specimen: a single paraphrased line in a handoff became a whole binary, a README-edit recommendation, and an argument for both.*
- If the brief doesn't settle it, stop and ask: Never fill a gap silently. An agent at full momentum that stops at the judgment boundary and asks for the line is doing the most valuable thing it can do.


# TESTING
- A test that cannot fail is not a test: Write RED tests, then make them pass.
- If you're asserting that a system works a certain way, write a test that fails if it doesn't.
- A verdict through `grep` is not a gate. `cargo test | tee` returns tee's status, not cargo's; a `&&` following a `grep` passed on nothing.

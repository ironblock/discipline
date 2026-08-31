# PRINCIPLES

- Truth over validation: it doesn't matter _who_ is right, it matters _what_ is right.
- Attribution over summary: a pointer to a line in a file is more durable than a description.
- Trust, but verify: assertions should be testable, and the documentation should make testing easy.
- Reject a faulty premise: is X the best or only way to achieve Y?
- Don't invent identifiers: prefer semantic descriptions over invented IDs, prefer IDs to come from a registry.
- Code and design: change as much as neccessary, and as little as possible.
- Be concise.

# TICKETS
- Editing a description is more visible than adding a comment - tools, agents, and people may only look at the body.

# GIT
- Don't work on `main`. Create semantic branches <feat|chore|fix>/<short-description> and merge via PR.
- PRs merge to main with `--no-ff` to preserve the branch history.

# README.MD / AGENTS.MD
- Progressive disclosure: don't frontload everything in a root directory, provide the information in the directory where it becomes relevant.

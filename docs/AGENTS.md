# Editing `docs/`

These files are the program's state. Their history is source control (`git log -p`, `git blame`), not an issue thread. Two kinds of line, two rules:

- **Evidence lines change only in the pull request that lands the evidence.** A ticked box in `releases.md`, a row's word in `experiments.md`, a lever's *current* state in `program.md`: each changes in the same diff as the results directory or merged change that makes it true, and cites it.
- **Thesis lines are the maintainer's.** The premise, what a lever means, a release's definition of done, an `[unsettled]` line becoming settled. An agent may propose one in a pull request; the maintainer's merge ratifies it, so the maintainer merges any pull request that changes one. No agent opens a pull request whose only content is rewording thesis lines.

And for both:
- **Every line cites a file, a results directory or a commit.** A cite to an issue comment is a pointer for history, kept until the line is rewritten from the record. Threads are not migrated in bulk: extraction as a chore has a measured fabrication rate (`AGENTS.md`, Documentation).
- **The why goes in the commit message.** No changelog, "as of" or "updated" lines inside a document.
- **Short on purpose.** A lever gets a paragraph; anything longer is a link.

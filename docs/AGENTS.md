# Editing `docs/`

These files are the program's state. Their history is source control (`git log -p`, `git blame`), not an issue thread. Every line is one of two kinds.

**Evidence lines** say what has been shown or built: a row's evidence cell in `releases.md`, a row's word in `experiments.md`, a lever's *current* column and what `program.md` says is or is not built. One changes in the pull request that lands the change or results directory making it true, or in a later one that cites it. A pull request that makes such a line false updates it (`.github/PULL_REQUEST_TEMPLATE.md`).

**Thesis lines** are every other line: the premise, what a lever means, a release's definition of done and its rows, a trajectory's script and stop conditions, the orchestration rulings, and this file. They are the maintainer's.
- A pull request that changes a thesis line is merged by the maintainer himself. No session merges it or arms auto-merge on it, and the pull request says in its body which thesis lines it changes.
- One GitHub identity posts for every session (`workflow.md`), so nothing on GitHub can show who pressed merge. This is a rule every session keeps, not a mechanism.

And for both:
- **A line of fact cites its source:** a file, a results directory, a commit, or an issue or pull request number as a pointer. A cite to an issue comment is kept until the line is rewritten from the record; threads are not migrated in bulk, because extraction as a chore has a measured fabrication rate (`AGENTS.md`, Documentation).
- **Provenance and the why go in the commit message.** No changelog, "as of", "compiled by" or "updated" lines inside a document.
- **Short on purpose.** A lever gets a paragraph; anything longer is a link.

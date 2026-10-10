# Editing `docs/`

These files are the program's state. Their history is source control (`git log -p`, `git blame`), not an issue thread. Any session may write them. What a line may say depends on its kind.

**Evidence lines** say what has been shown or built: a row's evidence cell in `releases.md`, a row's word in `experiments.md`, a lever's *current* column and what `program.md` says is or is not built. One changes in the pull request that lands the change or results directory making it true, or in a later one that cites it. A pull request that makes such a line false updates it (`.github/PULL_REQUEST_TEMPLATE.md`).

**Thesis lines** are every other line: the premise, what a lever means, an initiative, a milestone's definition and its rows, a trajectory's script and stop conditions, the maintainer's rulings, and this file. A change to one is a significant decision, so it needs the maintainer's approval, in any form, before it merges. The pull request names the thesis lines it changes and where he approved them. No session writes that the maintainer ruled, decided or wants something without his words.

**The predecessor is intent, never a result.** Anything measured in diet-inference records what it set out to do, never a conclusive finding (the maintainer, 2026-10-09: "I would treat everything in diet inference as 'what we wanted to do', never as a conclusive result"). Its harness differed: tool calls were code written in fenced Markdown blocks, so an interview that wrote a code block was scored "mimicry". A line that cites a predecessor measurement says it is the predecessor's, and nothing here rests on one.

And for both:
- **A line of fact cites its source:** a file, a results directory, a commit, or an issue or pull request number as a pointer. A cite to an issue comment is kept until the line is rewritten from the record. Threads are not migrated in bulk.
- **Provenance and the why go in the commit message.** No changelog, "as of", "compiled by" or "updated" lines inside a document.
- **Short on purpose.** A lever gets a paragraph; anything longer is a link.

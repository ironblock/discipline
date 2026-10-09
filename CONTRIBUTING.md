# TEMPLATES
All found in `.github/ISSUE_TEMPLATE/`:
  - `claim.md`: a hypothesis with a result
  - `defect.md`: for a reproduction with an expected/observed pair, 
  - `work.md` for anything built. Acceptance is a command and its exit code in all three.


# DOING SCIENCE
- Isolate a variable before declaring its impact. It will always be possible to run the test again.
- An idea can't be validated or discarded if it can't be instrumented and reproduced.


# BRANCHES AND RELEASES
Two long-lived branches, named once in `.github/branches.tsv` (#326):
- **The integration branch** (`develop`) is the repository's default branch and the base of every pull request.
- **The release branch** (`main`) takes only release pull requests from the integration branch. Nothing is versioned: a release is a tag on the release branch, not a version bump.

What runs: every pull request, and every push to either branch, runs `verify.yml`: the checks `.github/check-owners.tsv` assigns to each job, which are `verify.sh`'s `DEFAULT_CHECKS` -- conventional checks (fmt, clippy, tests, the surface's suite, hygiene, history) and the product's own data (the results records and regimens parse; the published pages' hygiene). Its `gate` job passes when every job succeeded. A newer push supersedes a run in flight, except on the way into the release branch. Two more checks re-derive committed data and run only by hand: `./verify.sh --only recompute` and `./verify.sh --only admission`. The seeded-fault selftest and the checks that guarded it were deleted (#508). **Pages** publishes from the integration branch: the site is the working surface.

Who cuts a release: the maintainer, as a pull request from the integration branch into the release branch, merged with `--no-ff` once its run is green; the merge is tagged.

`origin/HEAD` is set when a repository is cloned and is never moved after that, so an existing clone runs `git remote set-head origin --auto` once after the default branch changes; until it does, `origin/HEAD` still names the old default.

Nothing that runs spells either branch, except where an expression cannot go: a trigger's `branches:` list and `verify.yml`'s `cancel-in-progress`. Workflows read `github.event.repository.default_branch`, and scripts read `origin/HEAD` or `.github/branches.tsv`.


# MILESTONES
One milestone per tag, in the order the tags will be cut (`v0.1.0`, `v0.2.0`, ...). A milestone is what its release pull request contains, and it closes when the tag lands. Drives and trajectories are deliverables inside a version, never milestones of their own; programs (`program:dogma`, `program:claims`, `program:gate`) are labels, because they span versions. Ruled on #25 (2026-10-04, planning on the maintainer's word: comments 5976434649 and 5976773552), applied in one pass by Dispatch and recorded there (5976838317). That a milestone is what its release pull request contains follows from *Branches and releases* above, not from the ruling.
- **A child's milestone is never later than its parent's**, and the parent closes with its last child. Move the children first.
- **An unmilestoned issue is backlog.** A version adopts it at a cut, or it ages out: an item no version has adopted after two cuts is closed "not adopted", and reopens with a version.
- **Planning names what a version contains; Dispatch applies it** with the platform's tools and records the pass on #25, telling every session that holds a moved ticket its new home in the move itself.

# PULL REQUESTS AND REVIEWS
- Don't work on the integration or release branch. Create semantic branches <feat|chore|fix>/<short-description> from the integration branch and merge via PR into it.
- PRs merge with `--no-ff` to preserve the branch history.
- **A merge made through the platform's API stamps the authenticated account as the merge commit's author. That is expected, and it is not provenance.** What a change is and who vouched for it live in the PR's review record and acceptance table; the author field of a merge commit says only which client pressed the button, and the history gate does not read it (#81).
- **A PR's title and body are content the history gate scans too, and it scans a snapshot.** `pull_request` events fire on `opened`, `synchronize`, and `reopened` -- never on `edited` -- so a fix made by editing the description after a push is real but unseen: the run that already fired scanned the description as it stood at push time, and nothing re-checks it until the next push. Fix the description before you push, not after; if you fix it after, the check will only agree once something else lands (found live on #83, which is why this line exists).
- A local run needs `python3` 3.10 or later first on PATH: the scripts use 3.10's syntax, and the 3.9 a stock Mac ships in `/usr/bin` cannot parse them.
- Before pushing, run `./verify.sh` (the checks CI runs), or the checks your change can reach with `./verify.sh --only CHECK ...`.
- **Every pull request is reviewed adversarially before it merges**, by an instance that did not write the change, briefed to find what would make the change misbehave or make a test give false confidence. The review is posted on the pull request, and each finding is fixed, answered, or deferred to an issue, in the PR body.
- **Hygiene: nothing from a private environment, anywhere in the tree or on a thread.** Hostnames, addresses, home paths, internal ticket identifiers and private nicknames, which the hygiene gate refuses by pattern; and a person's schedule, habits or whereabouts, which no pattern catches and every reviewer does. A machine's availability is a fact about the machine ("reserved", "down", "restored at <time>"), never about a person's time (#283).
- **Acceptance is a command and its exit code.** Issue and PR templates carry it as a field, not a checkbox.
- **A deferral routes its payload to the successor's spec**, not just the source's grave. A defect measured at zero in a tree scheduled to freeze routes to the successor.


# RETAINED DATA
- Findings from external use come back as anonymized aggregates and patterns, never as artifacts from anyone's employer. One sentence; it protects both sides.
- **The working tree is the context window; history is the archive.** Evict to history, never delete. A record is evidence, not machinery.

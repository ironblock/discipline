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
- **The integration branch** (`develop`) is the repository's default branch and the base of every pull request. Merging into it never waits for a full run.
- **The release branch** (`main`) takes only release pull requests from the integration branch. Nothing is versioned: a release is a tag on the release branch, not a version bump.

What runs where:
- **A pull request into the integration branch** (or into any branch but the release branch) runs a *scoped* selftest: the faults its diff reaches, plus every package job. Its newer push supersedes its older run.
- **A push to the integration branch** runs the full selftest on `develop_shards` (`.github/gate-budget.tsv`). The next push cancels it: it is background, not a gate. The census pull requests are scoped against is the newest full run that *completed* on the integration branch, a push or the nightly.
- **A release pull request and the push to the release branch** run the full selftest on `max_shards` and are never cancelled, so the release branch is green by construction. A release pull request is refused while any drift issue (`check:<name>`) is open.
- **The nightly** runs the full selftest on the integration branch on `max_shards`. It and every push open drift issues; a pull request never does.
- **Pages** publishes from the integration branch: the site is the working surface.

Who cuts a release: the maintainer, or Dispatch on the maintainer's word, as a pull request from the integration branch into the release branch. It is reviewed and armed like any other pull request once its full run is green, merged with `--no-ff`, and the merge is tagged.

Nothing that runs spells either branch: workflows read `github.event.repository.default_branch`, scripts read `origin/HEAD` or the table, and `scripts/check-ci-coverage.py` refuses a literal anywhere else in the workflows, `scripts/` and `verify.sh` (outside its seeded-fault bodies); code elsewhere -- `exercise/`, `diet/`, `substrates/` -- is not scanned, so a branch name there is a review finding. The exceptions are the places that cannot take an expression -- a trigger's `branches:` list and `verify.yml`'s `cancel-in-progress` -- and the check holds each to the table.


# PULL REQUESTS AND REVIEWS
- Don't work on the integration or release branch. Create semantic branches <feat|chore|fix>/<short-description> from the integration branch and merge via PR into it.
- PRs merge with `--no-ff` to preserve the branch history.
- **A merge made through the platform's API stamps the authenticated account as the merge commit's author. That is expected, and it is not provenance.** What a change is and who vouched for it live in the PR's review record and acceptance table; the author field of a merge commit says only which client pressed the button, and the history gate does not read it (#81).
- **A PR's title and body are content the history gate scans too, and it scans a snapshot.** `pull_request` events fire on `opened`, `synchronize`, and `reopened` -- never on `edited` -- so a fix made by editing the description after a push is real but unseen: the run that already fired scanned the description as it stood at push time, and nothing re-checks it until the next push. Fix the description before you push, not after; if you fix it after, the check will only agree once something else lands (found live on #83, which is why this line exists).
- `./verify.sh` before pushing; `./verify.sh --selftest` when touching the gate or a fixture.
- **A PR that adds or removes a seeded fault edits no shard plan and no fault count.** A hash of the fault's id picks its shard, and the red count is derived by `scripts/check-fault-manifest.py`, not kept in `tools/gate/faults.toml` (#108, #112). A new fault has no last-red commit on the base branch, so the PR that adds it runs it.
- **Acceptance is a command and its exit code.** Issue and PR templates carry it as a field, not a checkbox.
- **A deferral routes its payload to the successor's spec**, not just the source's grave. A defect measured at zero in a tree scheduled to freeze routes to the successor.


# RETAINED DATA
- Findings from external use come back as anonymized aggregates and patterns, never as artifacts from anyone's employer. One sentence; it protects both sides.
- **The working tree is the context window; history is the archive.** Evict to history, never delete. A record is evidence, not machinery.

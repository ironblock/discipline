# TEMPLATES
All found in `.github/ISSUE_TEMPLATE/`:
  - `claim.md`: a hypothesis with a result
  - `defect.md`: for a reproduction with an expected/observed pair, 
  - `work.md` for anything built. Acceptance is a command and its exit code in all three.


# DOING SCIENCE
- Isolate a variable before declaring its impact. It will always be possible to run the test again.
- An idea can't be validated or discarded if it can't be instrumented and reproduced.


# PULL REQUESTS AND REVIEWS
- Don't work on `main`. Create semantic branches <feat|chore|fix>/<short-description> and merge via PR.
- PRs merge to main with `--no-ff` to preserve the branch history.
- **A merge made through the platform's API stamps the authenticated account as the merge commit's author. That is expected, and it is not provenance.** What a change is and who vouched for it live in the PR's review record and acceptance table; the author field of a merge commit says only which client pressed the button, and the history gate does not read it (#81).
- **A PR's title and body are content the history gate scans too, and it scans a snapshot.** `pull_request` events fire on `opened`, `synchronize`, and `reopened` -- never on `edited` -- so a fix made by editing the description after a push is real but unseen: the run that already fired scanned the description as it stood at push time, and nothing re-checks it until the next push. Fix the description before you push, not after; if you fix it after, the check will only agree once something else lands (found live on #83, which is why this line exists).
- `./verify.sh` before pushing; `./verify.sh --selftest` when touching the gate or a fixture.
- **Acceptance is a command and its exit code.** Issue and PR templates carry it as a field, not a checkbox.
- **A deferral routes its payload to the successor's spec**, not just the source's grave. A defect measured at zero in a tree scheduled to freeze routes to the successor.


# RETAINED DATA
- Findings from external use come back as anonymized aggregates and patterns, never as artifacts from anyone's employer. One sentence; it protects both sides.
- **The working tree is the context window; history is the archive.** Evict to history, never delete. A record is evidence, not machinery.

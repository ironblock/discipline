import { compose } from './compose.ts';
import type { Script } from './compose.ts';
import type { Recording } from './recorded.ts';

/**
 * THE KITCHEN SINK: one session at the cadence the surface expects of a
 * working drive -- the happy path, where the recordings are the floor.
 * Authored, not recorded, and composed onto a clock (compose.ts), so its
 * numbers are a hypothesis to measure a real drive against, not a record.
 *
 * Six asks over three phases (spec, build, review) and two refills. Side
 * calls run where #31 wants them: an extraction while a tool runs, an
 * interview in the person's gap after a turn settles, a ratify before each
 * refill -- about two per ask, each landing a few entries. One test run
 * fails and is fixed; nothing answers as the agent.
 *
 * The story: the operator wants `tally --top N`, the N largest files.
 */

const REPO = `You are a coding agent working in /work/tally, a Rust command-line tool that counts words, lines and bytes.
You have one tool, bash. Run one command at a time and read its output before the next.`;

const SORT_HITS = `src/report.rs:18:impl Report {
src/report.rs:22:    pub fn sorted(&self) -> Vec<&Tally> {
src/report.rs:23:        let mut v: Vec<&Tally> = self.tallies.iter().collect();
src/report.rs:24:        v.sort_by(|a, b| a.file.cmp(&b.file));
src/main.rs:31:    let report = Report::from_paths(&args.paths)?;`;

const REPORT_18_60 = `impl Report {
    pub fn from_paths(paths: &[PathBuf]) -> io::Result<Self> { … }

    pub fn sorted(&self) -> Vec<&Tally> {
        let mut v: Vec<&Tally> = self.tallies.iter().collect();
        v.sort_by(|a, b| a.file.cmp(&b.file));
        v
    }

    pub fn total(&self) -> Tally {
        self.tallies.iter().fold(Tally::default(), |acc, t| acc + t)
    }

    pub fn print(&self, args: &Args) -> io::Result<()> {
        let rows = self.sorted();
        …
    }
}`;

const TOP_DIFF = `git apply <<'EOF'
--- a/src/cli.rs
+++ b/src/cli.rs
@@ -33,3 +33,6 @@ pub struct Args {
     pub json: bool,
+    /// Only the N largest files by bytes; the total still covers every file
+    #[arg(long, value_name = "N")]
+    pub top: Option<usize>,
 }
--- a/src/report.rs
+++ b/src/report.rs
@@ -21,6 +21,13 @@ impl Report {
+    pub fn top(&self, n: usize) -> Vec<&Tally> {
+        let mut v: Vec<&Tally> = self.tallies.iter().collect();
+        v.sort_by(|a, b| b.bytes.cmp(&a.bytes));
+        v.truncate(n);
+        v
+    }
EOF`;

const TEST_FAILS = `running 11 tests
.........F.
failures:

---- report::tests::top_breaks_ties_by_name stdout ----
assertion failed: left == right
  left: ["b.txt", "a.txt"]
 right: ["a.txt", "b.txt"]

test result: FAILED. 10 passed; 1 failed; 0 ignored`;

const TIE_FIX = `sed -i 's/v.sort_by(|a, b| b.bytes.cmp(\\&a.bytes));/v.sort_by(|a, b| b.bytes.cmp(\\&a.bytes).then_with(|| a.file.cmp(\\&b.file)));/' src/report.rs`;

const TESTS_PASS = (n: number) => `running ${n} tests
${'.'.repeat(n)}
test result: ok. ${n} passed; 0 failed; 0 ignored; finished in 0.05s`;

const SPEC_TO_BUILD = `${REPO}

Current work phase: build.
Implement what the spec phase decided. Run the tests before you finish.

WORKING MEMORY (render v1)

Decisions
#d1 --top N is an Option<usize> on Args; absent means every file
#d2 largest by bytes; ties broken by file name
#d3 the total still covers every file, not just the top N

Facts
#f1 rows are ordered in Report::sorted (src/report.rs:22-26), by name
#f2 Report::print takes its rows from sorted() (src/report.rs:31)

Next
#n1 add Report::top beside sorted(), use it in print when --top is set`;

const BUILD_TO_REVIEW = `${REPO}

Current work phase: review.
Read the change as a maintainer would before it merges. Say what to fix; fix nothing unasked.

WORKING MEMORY (render v2)

Decisions
#d1 --top N is an Option<usize> on Args; absent means every file
#d2 largest by bytes; ties broken by file name
#d3 the total still covers every file, not just the top N
#d4 --top applies to --json the same way: N objects, then the total

Facts
#f3 Report::top sorts by bytes, then name (src/report.rs:22-28)
#f4 11 tests pass, one per ordering rule

Next
#n2 review the diff before merge`;

export const KITCHEN_SINK_SCRIPT: Script = {
  model: 'a 27B instruct model, Q4, one 24 GB GPU',
  slots: 3,
  phase: 'spec',
  system: `${REPO}

Current work phase: spec.
Agree what to build with the operator before changing anything. End the phase with a short, numbered proposal and any open question.`,
  parts: [
    {
      kind: 'turn',
      after: 1500,
      ask: 'I want tally to print only the N largest files. Look at how rows are ordered and printed, and propose a --top flag.',
      steps: [
        {
          think: 'Find where rows are ordered before reading anything whole.',
          say: 'Finding where rows are ordered.',
          run: { command: 'rg -n "sort" src', output: SORT_HITS, ms: 60 },
        },
        {
          think: 'Report::sorted orders by name and print takes its rows from it. Read just that impl.',
          say: 'Reading the impl that orders and prints rows.',
          run: { command: 'sed -n 18,60p src/report.rs', output: REPORT_18_60, ms: 30 },
        },
        {
          think: 'A --top N would take the largest files by bytes. Two open points: how to break ties, and whether the total covers only the top N.',
          say: "Rows are ordered in one place, `Report::sorted` (src/report.rs:22-26), by name, and `print` takes them from it.\n\nProposal:\n\n1. Add `--top N` to `Args` as an optional count.\n2. Add `Report::top(n)` beside `sorted`: largest by bytes, first N.\n3. Use it in `print` when the flag is set.\n\nTwo questions: how should ties break, and should the total cover every file or only the N shown?",
        },
      ],
      sides: [
        {
          lane: 'extraction',
          slot: 2,
          when: 1,
          why: 'a read of report.rs landed; extract what it established',
          question: 'From the output you just read, what facts will matter later? FACT: lines, with file and line.',
          answer: 'FACT: rows are ordered in Report::sorted (src/report.rs:22-26), by name\nFACT: Report::print takes its rows from sorted() (src/report.rs:31)',
          changes: [
            { op: 'add', id: 'f1', category: 'Facts', text: 'rows are ordered in Report::sorted (src/report.rs:22-26), by name', authority: 'extracted' },
            { op: 'add', id: 'f2', category: 'Facts', text: 'Report::print takes its rows from sorted() (src/report.rs:31)', authority: 'extracted' },
          ],
        },
        {
          lane: 'interview',
          slot: 1,
          when: 'settled',
          why: 'the turn settled with a proposal and two questions to the operator',
          question: 'What did you decide, and what is still open? DECISION: and OPEN: lines.',
          answer: 'DECISION: --top N is an Option<usize> on Args; absent means every file\nOPEN: how ties break\nOPEN: whether the total covers every file or only the top N',
          changes: [
            { op: 'add', id: 'd1', category: 'Decisions', text: '--top N is an Option<usize> on Args; absent means every file' },
            { op: 'add', id: 'o1', category: 'Open', text: 'how ties break' },
            { op: 'add', id: 'o2', category: 'Open', text: 'whether the total covers every file or only the top N' },
          ],
        },
      ],
    },
    {
      kind: 'turn',
      after: 38_000,
      ask: 'Largest by bytes, ties by file name. The total should still cover everything.',
      steps: [
        {
          think: 'Both questions answered. Restate the spec in one place.',
          say: 'Agreed:\n\n- `--top N` shows the N largest files by bytes; ties go to the earlier file name.\n- The total still covers every file.\n- Without `--top`, nothing changes.\n\nReady to build when you move to it.',
        },
      ],
      sides: [
        {
          lane: 'interview',
          slot: 1,
          when: 'settled',
          why: 'the operator answered both OPEN questions',
          question: 'Anything to record? DECISION:, CONSTRAINT: or SUPERSEDE: lines.',
          answer: 'SUPERSEDE: OPEN how ties break -> DECISION: largest by bytes; ties broken by file name\nSUPERSEDE: OPEN whether the total covers every file -> DECISION: the total still covers every file, not just the top N',
          changes: [
            { op: 'supersede', id: 'd2', category: 'Decisions', text: 'largest by bytes; ties broken by file name', supersedes: 'o1', authority: 'stated' },
            { op: 'supersede', id: 'd3', category: 'Decisions', text: 'the total still covers every file, not just the top N', supersedes: 'o2', authority: 'stated' },
          ],
        },
      ],
    },
    {
      kind: 'refill',
      after: 21_000,
      phase: { from: 'spec', to: 'build' },
      ratify: {
        slot: 1,
        why: 'the operator declared spec → build',
        question: 'Here is the working memory this phase built. Does it cohere? KEEP, UPDATE or REMOVE per entry, then ADD anything missing.',
        answer: 'KEEP d1 d2 d3 f1 f2\nADD NEXT: add Report::top beside sorted(), use it in print when --top is set',
        changes: [{ op: 'add', id: 'n1', category: 'Next', text: 'add Report::top beside sorted(), use it in print when --top is set' }],
      },
      render: SPEC_TO_BUILD,
    },
    {
      kind: 'turn',
      after: 6_000,
      ask: 'Go ahead.',
      steps: [
        {
          think: 'Working memory has the plan and the lines. Add the flag and Report::top in one patch.',
          say: 'Adding the flag and `Report::top`.',
          run: { command: TOP_DIFF, output: '', ms: 80 },
        },
        {
          think: 'Applied. Add a test per ordering rule and run them.',
          say: 'Running the tests, with one per ordering rule.',
          run: { command: 'cargo test --quiet', output: TEST_FAILS, ms: 14_200, exit: 101 },
        },
        {
          think: 'The tie test fails: sort_by on bytes alone keeps input order for equal sizes. Break ties by name.',
          say: 'Ties keep input order. Breaking them by name.',
          run: { command: TIE_FIX, output: '', ms: 40 },
        },
        {
          say: 'Running the tests again.',
          run: { command: 'cargo test --quiet', output: TESTS_PASS(11), ms: 6_100 },
        },
        {
          think: 'All eleven pass. Summarise.',
          say: 'Done.\n\n- `--top N` prints the N largest files by bytes, ties by name.\n- The total still covers every file.\n- Three new tests; all 11 pass.',
        },
      ],
      sides: [
        {
          lane: 'extraction',
          slot: 2,
          when: 1,
          why: 'cargo test is running: the trunk is idle for a while',
          question: 'What did you just change? FACT: lines, citing files and lines.',
          answer: 'FACT: --top added to Args as Option<usize> (src/cli.rs:34-36)\nFACT: Report::top sorts by bytes and truncates to N (src/report.rs:22-27)',
          changes: [
            { op: 'add', id: 'f3', category: 'Facts', text: 'Report::top sorts by bytes and truncates to N (src/report.rs:22-27)', authority: 'extracted' },
          ],
        },
        {
          lane: 'interview',
          slot: 1,
          when: 'settled',
          why: 'a test failed and was fixed in the turn',
          question: 'What changed after the failure, and is anything left to do? FACT: or NEXT: lines.',
          answer: 'SUPERSEDE: FACT f3 -> FACT: Report::top sorts by bytes, then name (src/report.rs:22-28)\nFACT: 11 tests pass, one per ordering rule',
          changes: [
            { op: 'supersede', id: 'f3b', category: 'Facts', text: 'Report::top sorts by bytes, then name (src/report.rs:22-28)', supersedes: 'f3', authority: 'observed' },
            { op: 'add', id: 'f4', category: 'Facts', text: '11 tests pass, one per ordering rule', authority: 'observed' },
            { op: 'resolve', id: 'n1', category: 'Next', text: 'add Report::top beside sorted(), use it in print when --top is set -- done' },
          ],
        },
      ],
    },
    {
      kind: 'turn',
      after: 44_000,
      ask: 'Make --top work with --json too.',
      steps: [
        {
          think: 'print takes rows from one place; the JSON branch reads self.tallies directly. Point it at the same rows.',
          say: 'The JSON branch reads every tally directly. Pointing it at the same rows.',
          run: { command: "sed -i 's/for t in &self.tallies {/for t in &rows {/' src/report.rs", output: '', ms: 40 },
        },
        {
          say: 'Running the tests.',
          run: { command: 'cargo test --quiet', output: TESTS_PASS(12), ms: 6_300 },
        },
        {
          say: '`--top` now applies to `--json`: N objects, then the total over every file. One new test; all 12 pass.',
        },
      ],
      sides: [
        {
          lane: 'extraction',
          slot: 2,
          when: 1,
          why: 'cargo test is running: the trunk is idle for a while',
          question: 'What did you just change? FACT: lines, citing files and lines.',
          answer: 'FACT: the JSON branch of Report::print iterates the same rows as the table (src/report.rs:45)',
          changes: [{ op: 'add', id: 'f5', category: 'Facts', text: 'the JSON branch of Report::print iterates the same rows as the table (src/report.rs:45)', authority: 'extracted' }],
        },
        {
          lane: 'interview',
          slot: 1,
          when: 'settled',
          why: 'the operator extended the spec in a build turn',
          question: 'Anything to record? DECISION: or CONSTRAINT: lines.',
          answer: 'DECISION: --top applies to --json the same way: N objects, then the total',
          changes: [{ op: 'add', id: 'd4', category: 'Decisions', text: '--top applies to --json the same way: N objects, then the total', authority: 'stated' }],
        },
      ],
    },
    {
      kind: 'refill',
      after: 18_000,
      phase: { from: 'build', to: 'review' },
      ratify: {
        slot: 1,
        why: 'the operator declared build → review',
        question: 'Here is the working memory this phase built. Does it cohere? KEEP, UPDATE or REMOVE per entry, then ADD anything missing.',
        answer: 'KEEP d1 d2 d3 d4 f3b f4\nREMOVE f1 f2 -- sorted() is no longer where --top is decided\nREMOVE f5 -- covered by d4\nADD NEXT: review the diff before merge',
        changes: [
          { op: 'retire', id: 'f1', category: 'Facts', text: 'rows are ordered in Report::sorted (src/report.rs:22-26), by name' },
          { op: 'retire', id: 'f2', category: 'Facts', text: 'Report::print takes its rows from sorted() (src/report.rs:31)' },
          { op: 'retire', id: 'f5', category: 'Facts', text: 'the JSON branch of Report::print iterates the same rows as the table (src/report.rs:45)' },
          { op: 'add', id: 'n2', category: 'Next', text: 'review the diff before merge' },
        ],
      },
      render: BUILD_TO_REVIEW,
    },
    {
      kind: 'turn',
      after: 5_000,
      ask: 'Review the change as a maintainer would. Anything to fix before I merge?',
      steps: [
        {
          think: 'Read the whole diff once.',
          say: 'Reading the diff.',
          run: { command: 'git diff --stat && git diff', output: `${TOP_DIFF.split('\n').slice(1, -1).join('\n')}`, ms: 50 },
        },
        {
          think: "The logic matches the decisions. The doc comment on --top says 'largest files' without saying by bytes, and --top 0 prints only the total, which may surprise.",
          say: 'Two things before merge:\n\n1. The doc comment on `--top` should say *by bytes*.\n2. `--top 0` prints only the total. Reject it, or keep it?\n\nEverything else matches the decisions.',
        },
      ],
      sides: [
        {
          lane: 'interview',
          slot: 1,
          when: 'settled',
          why: 'the review ended with a question to the operator',
          question: 'What is still open? OPEN: lines.',
          answer: 'OPEN: whether --top 0 is rejected or prints only the total',
          changes: [{ op: 'add', id: 'o3', category: 'Open', text: 'whether --top 0 is rejected or prints only the total' }],
        },
      ],
    },
    {
      kind: 'turn',
      after: 29_000,
      ask: 'Reject 0, fix the doc comment, and we are done.',
      steps: [
        {
          think: 'clap can reject 0 with a range parser; fix the comment in the same edit.',
          say: 'Rejecting 0 and fixing the comment.',
          run: {
            command: "sed -i 's|Only the N largest files by bytes|Only the N largest files, by bytes (N ≥ 1)|; s|#\\[arg(long, value_name = \"N\")\\]|#[arg(long, value_name = \"N\", value_parser = clap::value_parser!(usize).range(1..))]|' src/cli.rs",
            output: '',
            ms: 40,
          },
        },
        {
          say: 'Running the tests.',
          run: { command: 'cargo test --quiet', output: TESTS_PASS(13), ms: 6_400 },
        },
        {
          say: 'Done: `--top 0` is rejected with a usage error, the comment says *by bytes*, and a test covers the rejection. All 13 pass.',
        },
      ],
      sides: [
        {
          lane: 'extraction',
          slot: 2,
          when: 1,
          why: 'cargo test is running: the trunk is idle for a while',
          question: 'What did you just change? FACT: lines, citing files and lines.',
          answer: 'FACT: --top rejects 0 with a range parser (src/cli.rs:35)',
          changes: [{ op: 'add', id: 'f6', category: 'Facts', text: '--top rejects 0 with a range parser (src/cli.rs:35)', authority: 'extracted' }],
        },
        {
          lane: 'interview',
          slot: 1,
          when: 'settled',
          why: 'the operator answered the OPEN question',
          question: 'Anything to record? DECISION: or RESOLVE: lines.',
          answer: 'RESOLVE: OPEN whether --top 0 is rejected -> rejected with a usage error\nRESOLVE: NEXT review the diff -> reviewed; two fixes made',
          changes: [
            { op: 'resolve', id: 'o3', category: 'Open', text: 'whether --top 0 is rejected or prints only the total -- rejected with a usage error', authority: 'stated' },
            { op: 'resolve', id: 'n2', category: 'Next', text: 'review the diff before merge -- reviewed; two fixes made' },
          ],
        },
      ],
    },
  ],
};

export const KITCHEN_SINK: Recording = {
  title: 'The kitchen sink: a happy path, authored',
  migration: [
    'authored, not recorded: composed from a script (src/drive/kitchen-sink.ts) onto a clock (compose.ts)',
    'token counts from text length; prefill and decode from assumed rates, not measured',
    'each tool call says where it began (calls_from), as a drive calling tools natively can; the recordings cannot',
  ],
  carried: {},
  events: compose(KITCHEN_SINK_SCRIPT),
};

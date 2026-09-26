/**
 * THE SPECIMEN: one authored session, walking #31's definition of done.
 *
 * This is not a record and nothing validated it. It is hand-written, in the
 * provisional vocabulary of `events.ts`, to show what the surface needs the
 * drive to say -- the forcing function, in data. Its numbers are plausible
 * for one local model on llama.cpp with `-np 2`, and are not measurements.
 * When #117's loop runs, a recorded session replaces it and this file is
 * deleted.
 *
 * The story: the operator asks for a `--json` flag. The model reads a
 * 1,860-line file to find forty-nine lines (the aberration `diet` exists to
 * notice); an interview in the idle gap keeps what mattered; the spec
 * settles; the operator declares spec → build; ratify, render, refill; the
 * build turn starts from 1.5k tokens of working memory instead of 17.9k of
 * transcript, and reads only the forty-nine lines it needs.
 */

import type { Timings, Unplaced } from './events.ts';

/** What starts a beat: the session opening, a person's ask, a declared seam. */
export type Trigger = 'open' | 'send' | 'seam';

/** One stretch of a session, started by one action. `t` is relative to the beat's start. */
export interface Beat {
  readonly trigger: Trigger;
  readonly events: readonly Unplaced[];
}

const timings = (prompt_n: number, cache_n: number, prompt_ms: number, predicted_n: number, predicted_ms: number): Timings => ({
  prompt_n,
  cache_n,
  prompt_ms,
  predicted_n,
  predicted_ms,
});

const SYSTEM_SPEC = `You are a coding agent working in /work/tally, a Rust command-line tool that counts words, lines and bytes.
You have one tool, bash. Run one command at a time and read its output before the next.

Current work phase: spec.
Agree what to build with the operator before changing anything. End the phase with a short, numbered proposal and any open question.`;

const RENDER_BUILD = `You are a coding agent working in /work/tally, a Rust command-line tool that counts words, lines and bytes.
You have one tool, bash. Run one command at a time and read its output before the next.

Current work phase: build.
Implement what the spec phase decided. Run the tests before you finish.

WORKING MEMORY (render v1)

Decisions
#d1 --json is a flag on Args; the table stays the default
#d2 reuse Tally's Serialize; no new data model
#d3 JSON Lines: one object per file, then a totals line

Constraints
#c1 field names match the table headers, lower-cased

Facts
#f1 output format is decided only in Report::print (src/report.rs:40-88)
#f2 Tally { words, lines, bytes } derives serde::Serialize (src/report.rs:12)

Next
#n1 implement --json in Args and Report::print, then a test per shape`;

/** `cat src/report.rs`: 1,860 lines, of which the turn needed 40-88. */
function reportRs(): string {
  const head = [
    'use std::io::{self, Write};',
    '',
    'use comfy_table::{presets::UTF8_FULL, Cell, Color, Table};',
    'use serde::Serialize;',
    '',
    'use crate::cli::Args;',
    '',
    '/// Counts for one input.',
    '#[derive(Debug, Clone, Default, Serialize)]',
    'pub struct Tally {',
    '    pub file: String,',
    '    pub words: u64,',
    '    pub lines: u64,',
    '    pub bytes: u64,',
    '}',
    '',
  ];
  const lines = [...head];
  for (let i = lines.length + 1; i <= 1860; i += 1) {
    const n = i % 23;
    lines.push(
      n === 0
        ? `fn column_width_${i}(cells: &[Cell], min: usize) -> usize {`
        : n === 1
          ? '    cells.iter().map(|c| c.content().chars().count()).max().unwrap_or(min).max(min)'
          : n === 2
            ? '}'
            : n === 3
              ? ''
              : n < 8
                ? `    // colour rule ${i}: dim zero counts, highlight the largest file`
                : n < 16
                  ? `    let cell_${i} = Cell::new(format!("{:>w$}", value, w = width)).fg(Color::Grey);`
                  : `    table.add_row(vec![cell_${i - 1}.clone(), cell_${i}.clone()]);`,
    );
  }
  return lines.join('\n');
}

const REPORT_40_88 = `impl Report {
    pub fn print(&self, args: &Args) -> io::Result<()> {
        let mut out = io::stdout().lock();
        let mut table = Table::new();
        table.load_preset(UTF8_FULL);
        if !args.no_header {
            table.set_header(vec!["File", "Words", "Lines", "Bytes"]);
        }
        for t in &self.tallies {
            table.add_row(vec![
                Cell::new(&t.file),
                Cell::new(t.words),
                Cell::new(t.lines),
                Cell::new(t.bytes),
            ]);
        }
        if self.tallies.len() > 1 {
            let total = self.total();
            table.add_row(vec![
                Cell::new("total"),
                Cell::new(total.words),
                Cell::new(total.lines),
                Cell::new(total.bytes),
            ]);
        }
        writeln!(out, "{table}")
    }
    // … 24 more lines of column and colour handling`;

const JSON_DIFF = `cat > /tmp/json.diff <<'EOF'
--- a/src/cli.rs
+++ b/src/cli.rs
@@ -29,4 +29,7 @@ pub struct Args {
     #[arg(long)]
     pub no_header: bool,
+    /// One JSON object per file, then a totals line (JSON Lines)
+    #[arg(long, conflicts_with = "no_header")]
+    pub json: bool,
 }
--- a/src/report.rs
+++ b/src/report.rs
@@ -41,6 +41,16 @@ impl Report {
     pub fn print(&self, args: &Args) -> io::Result<()> {
         let mut out = io::stdout().lock();
+        if args.json {
+            for t in &self.tallies {
+                serde_json::to_writer(&mut out, t)?;
+                writeln!(out)?;
+            }
+            let total = self.total();
+            serde_json::to_writer(&mut out, &serde_json::json!({ "total": total }))?;
+            return writeln!(out);
+        }
         let mut table = Table::new();
EOF
git apply /tmp/json.diff`;

export const SPECIMEN: readonly Beat[] = [
  {
    trigger: 'open',
    events: [
      {
        kind: 'session.start',
        t: 0,
        arm: 'diet',
        model: 'a 27B instruct model, Q4, one 24 GB GPU',
        slots: 2,
        trunk_slot: 0,
        phase: 'spec',
        system: { text: SYSTEM_SPEC, tokens: 1184 },
      },
    ],
  },
  {
    trigger: 'send',
    events: [
      {
        kind: 'ask',
        t: 0,
        turn: 1,
        text: 'tally prints its counts as a table. I want a --json flag so I can pipe it into jq. Before changing anything, look at how the CLI is put together and tell me what you would propose.',
      },
      { kind: 'request', t: 30, id: 'q/1', lane: 'trunk', slot: 0, turn: 1 },
      {
        kind: 'response',
        t: 2680,
        id: 'q/1#response',
        to_request: 'q/1',
        reasoning: 'The operator wants a --json flag. Before proposing anything I need to know where arguments are parsed and where output is printed. Start with the layout of src/.',
        text: 'Let me look at how the CLI is laid out first.',
        stop: 'tool',
        timings: timings(1236, 0, 820, 64, 1830),
      },
      { kind: 'tool.begin', t: 2700, id: 't/1', turn: 1, after: 'q/1#response', tool: 'bash', args: { command: 'ls src && wc -l src/*.rs' } },
      {
        kind: 'tool.end',
        t: 2740,
        id: 't/1',
        exit: 0,
        output: 'cli.rs\nmain.rs\nreport.rs\n   212 src/cli.rs\n    48 src/main.rs\n  1860 src/report.rs\n  2120 total',
      },
      { kind: 'request', t: 2760, id: 'q/2', lane: 'trunk', slot: 0, turn: 1 },
      {
        kind: 'response',
        t: 4010,
        id: 'q/2#response',
        to_request: 'q/2',
        reasoning: 'report.rs is by far the largest file; the printing is almost certainly there. Read it.',
        text: 'Output happens in report.rs. Reading it.',
        stop: 'tool',
        timings: timings(108, 1300, 90, 41, 1150),
      },
      { kind: 'tool.begin', t: 4030, id: 't/2', turn: 1, after: 'q/2#response', tool: 'bash', args: { command: 'cat src/report.rs' } },
      { kind: 'tool.end', t: 4045, id: 't/2', exit: 0, output: reportRs() },
      { kind: 'request', t: 4060, id: 'q/3', lane: 'trunk', slot: 0, turn: 1 },
      {
        kind: 'response',
        t: 27600,
        id: 'q/3#response',
        to_request: 'q/3',
        reasoning:
          'The file is 1,860 lines but almost all of it is column-width and colour handling. The format decision is made in one place, Report::print, lines 40-88, which builds a comfy_table and writes it. Tally is at line 12 and already derives serde::Serialize, so JSON needs no new data model. Arguments are clap derive in src/cli.rs; no_header is the only output flag today, and --json would contradict it. One real question for the operator: a single JSON document, or JSON Lines so it streams into jq.',
        text: "Here's what I found, and what I'd propose.\n\n- Output is decided in one place: `Report::print` (src/report.rs:40-88) builds a table with comfy_table.\n- `Tally` already derives `serde::Serialize`, so JSON needs no new data model.\n- Arguments are clap derive in src/cli.rs.\n\nProposal:\n\n1. Add `--json` to `Args`, conflicting with `--no-header`.\n2. In `Report::print`, write JSON when the flag is set and the table otherwise.\n3. Keep the table as the default; exit codes unchanged.\n\nOne question: should `--json` print one object for everything, or one line per file (JSON Lines) so it streams into jq?",
        stop: 'stop',
        timings: timings(16420, 1410, 11900, 402, 11600),
      },
      { kind: 'turn.settled', t: 27620, turn: 1, reason: 'final' },
      {
        kind: 'fork',
        t: 27700,
        id: 'i/1',
        lane: 'interview',
        slot: 1,
        of_turn: 1,
        at: 't/2',
        why: 'read 1,860 lines (16.4k tokens); the answer cites lines 12 and 40-88',
        question: 'You read all of src/report.rs. What from it will matter for the rest of this task? Answer with FACT: lines, citing line numbers.',
        prefix_tokens: 17830,
      },
      { kind: 'request', t: 27710, id: 'i/1/q', lane: 'interview', slot: 1, turn: 1, fork: 'i/1' },
      {
        kind: 'response',
        t: 29830,
        id: 'i/1/q#response',
        to_request: 'i/1/q',
        text: 'FACT: output format is decided only in Report::print (src/report.rs:40-88)\nFACT: Tally { words, lines, bytes } derives serde::Serialize (src/report.rs:12)\nFACT: the rest of report.rs is column-width and colour handling; not needed for --json',
        stop: 'stop',
        timings: timings(38, 17830, 60, 71, 2050),
      },
      { kind: 'fork.settled', t: 29840, id: 'i/1', outcome: 'complete' },
      { kind: 'patch', t: 29850, id: 'p/1', from: 'i/1', op: 'add', entry: { id: 'f1', category: 'Facts', text: 'output format is decided only in Report::print (src/report.rs:40-88)' } },
      { kind: 'patch', t: 29850, id: 'p/2', from: 'i/1', op: 'add', entry: { id: 'f2', category: 'Facts', text: 'Tally { words, lines, bytes } derives serde::Serialize (src/report.rs:12)' } },
      { kind: 'patch', t: 29850, id: 'p/3', from: 'i/1', op: 'add', entry: { id: 'f3', category: 'Facts', text: 'the rest of report.rs is column-width and colour handling; not needed for --json' } },
      {
        kind: 'fork',
        t: 29900,
        id: 'i/2',
        lane: 'interview',
        slot: 1,
        of_turn: 1,
        at: 'q/3#response',
        why: 'the turn settled with a proposal and an open question to the operator',
        question: 'What did you decide, and what is still open? Answer with DECISION: and OPEN: lines.',
        prefix_tokens: 18240,
      },
      { kind: 'request', t: 29910, id: 'i/2/q', lane: 'interview', slot: 1, turn: 1, fork: 'i/2' },
      {
        kind: 'response',
        t: 32100,
        id: 'i/2/q#response',
        to_request: 'i/2/q',
        text: "DECISION: --json is a flag on Args; the table stays the default\nDECISION: reuse Tally's Serialize; no new data model\nOPEN: one JSON document or JSON Lines per file -- asked the operator",
        stop: 'stop',
        timings: timings(31, 18240, 50, 64, 1880),
      },
      { kind: 'fork.settled', t: 32110, id: 'i/2', outcome: 'complete' },
      { kind: 'patch', t: 32120, id: 'p/4', from: 'i/2', op: 'add', entry: { id: 'd1', category: 'Decisions', text: '--json is a flag on Args; the table stays the default' } },
      { kind: 'patch', t: 32120, id: 'p/5', from: 'i/2', op: 'add', entry: { id: 'd2', category: 'Decisions', text: "reuse Tally's Serialize; no new data model" } },
      { kind: 'patch', t: 32120, id: 'p/6', from: 'i/2', op: 'add', entry: { id: 'o1', category: 'Open', text: 'one JSON document or JSON Lines per file' } },
    ],
  },
  {
    trigger: 'send',
    events: [
      { kind: 'ask', t: 0, turn: 2, text: "JSON Lines: one object per file, then a totals line. Match the table's field names." },
      { kind: 'request', t: 30, id: 'q/4', lane: 'trunk', slot: 0, turn: 2 },
      {
        kind: 'response',
        t: 3420,
        id: 'q/4#response',
        to_request: 'q/4',
        reasoning: 'That answers the open question. Restate the agreed spec so the build phase has it in one place.',
        text: 'Agreed:\n\n- `--json` writes JSON Lines: one object per file with `file`, `words`, `lines` and `bytes`, then `{"total": …}`.\n- Field names are the table headers, lower-cased.\n- Nothing else changes.\n\nThat settles the spec. When you move to build, I will implement it in `Args` and `Report::print` and add a test for each shape.',
        stop: 'stop',
        timings: timings(31, 17860, 40, 118, 3300),
      },
      { kind: 'turn.settled', t: 3440, turn: 2, reason: 'final' },
      {
        kind: 'fork',
        t: 3500,
        id: 'i/3',
        lane: 'interview',
        slot: 1,
        of_turn: 2,
        at: 'q/4#response',
        why: 'the operator answered an OPEN question',
        question: 'Anything you meant to record? Use DECISION:, CONSTRAINT: or SUPERSEDE: lines.',
        prefix_tokens: 18030,
      },
      { kind: 'request', t: 3510, id: 'i/3/q', lane: 'interview', slot: 1, turn: 2, fork: 'i/3' },
      {
        kind: 'response',
        t: 5320,
        id: 'i/3/q#response',
        to_request: 'i/3/q',
        text: 'SUPERSEDE: OPEN one JSON document or JSON Lines -> DECISION: JSON Lines: one object per file, then a totals line\nCONSTRAINT: field names match the table headers, lower-cased',
        stop: 'stop',
        timings: timings(29, 18030, 40, 52, 1560),
      },
      { kind: 'fork.settled', t: 5330, id: 'i/3', outcome: 'complete' },
      {
        kind: 'patch',
        t: 5340,
        id: 'p/7',
        from: 'i/3',
        op: 'supersede',
        supersedes: 'o1',
        entry: { id: 'd3', category: 'Decisions', text: 'JSON Lines: one object per file, then a totals line' },
      },
      { kind: 'patch', t: 5340, id: 'p/8', from: 'i/3', op: 'add', entry: { id: 'c1', category: 'Constraints', text: 'field names match the table headers, lower-cased' } },
    ],
  },
  {
    trigger: 'seam',
    events: [
      {
        kind: 'fork',
        t: 0,
        id: 'r/1',
        lane: 'ratify',
        slot: 1,
        of_turn: 2,
        at: 'q/4#response',
        why: 'the operator declared spec → build',
        question: 'Here is the working memory you built this phase. Does it cohere? Answer KEEP, UPDATE or REMOVE per entry, then ADD anything missing.',
        prefix_tokens: 18060,
      },
      { kind: 'request', t: 10, id: 'r/1/q', lane: 'ratify', slot: 1, turn: 2, fork: 'r/1' },
      {
        kind: 'response',
        t: 3900,
        id: 'r/1/q#response',
        to_request: 'r/1/q',
        text: 'KEEP f1 f2 d1 d2 d3 c1\nREMOVE f3 -- it only says what not to read\nADD NEXT: implement --json in Args and Report::print, then a test per shape',
        stop: 'stop',
        timings: timings(214, 18060, 180, 58, 1700),
      },
      { kind: 'fork.settled', t: 3910, id: 'r/1', outcome: 'complete' },
      { kind: 'patch', t: 3920, id: 'p/9', from: 'r/1', op: 'retire', entry: { id: 'f3', category: 'Facts', text: 'the rest of report.rs is column-width and colour handling; not needed for --json' } },
      { kind: 'patch', t: 3920, id: 'p/10', from: 'r/1', op: 'add', entry: { id: 'n1', category: 'Next', text: 'implement --json in Args and Report::print, then a test per shape' } },
      {
        kind: 'seam',
        t: 5100,
        id: 's/1',
        at_turn: 2,
        reason: 'operator',
        phase: { from: 'spec', to: 'build' },
        prefix_hash_before: 'a41c09e2',
        prefix_hash_after: '7e02b5d1',
        render: { version: 1, text: RENDER_BUILD, tokens: 1512 },
        warm: timings(1512, 0, 1050, 1, 30),
      },
    ],
  },
  {
    trigger: 'send',
    events: [
      { kind: 'ask', t: 0, turn: 3, text: 'Go ahead.' },
      { kind: 'request', t: 30, id: 'q/5', lane: 'trunk', slot: 0, turn: 3 },
      {
        kind: 'response',
        t: 1900,
        id: 'q/5#response',
        to_request: 'q/5',
        reasoning: 'Working memory says the format is decided in Report::print, lines 40-88. Read just those.',
        text: 'Reading the part of report.rs that decides the format.',
        stop: 'tool',
        timings: timings(12, 1512, 20, 48, 1380),
      },
      { kind: 'tool.begin', t: 1920, id: 't/3', turn: 3, after: 'q/5#response', tool: 'bash', args: { command: 'sed -n 40,88p src/report.rs' } },
      { kind: 'tool.end', t: 1935, id: 't/3', exit: 0, output: REPORT_40_88 },
      { kind: 'request', t: 1950, id: 'q/6', lane: 'trunk', slot: 0, turn: 3 },
      {
        kind: 'response',
        t: 9700,
        id: 'q/6#response',
        to_request: 'q/6',
        reasoning: 'Add the flag with conflicts_with = "no_header", and an early branch at the top of print that writes JSON Lines and a totals object.',
        text: 'Adding the flag and the JSON Lines branch.',
        stop: 'tool',
        timings: timings(468, 1572, 330, 252, 7300),
      },
      { kind: 'tool.begin', t: 9720, id: 't/4', turn: 3, after: 'q/6#response', tool: 'bash', args: { command: JSON_DIFF } },
      { kind: 'tool.end', t: 9790, id: 't/4', exit: 0, output: '' },
      { kind: 'request', t: 9800, id: 'q/7', lane: 'trunk', slot: 0, turn: 3 },
      {
        kind: 'response',
        t: 10900,
        id: 'q/7#response',
        to_request: 'q/7',
        reasoning: 'Applied cleanly. Run the tests.',
        text: 'Applied. Running the tests.',
        stop: 'tool',
        timings: timings(16, 2290, 20, 34, 1000),
      },
      { kind: 'tool.begin', t: 10920, id: 't/5', turn: 3, after: 'q/7#response', tool: 'bash', args: { command: 'cargo test --quiet' } },
      {
        kind: 'fork',
        t: 11000,
        id: 'i/4',
        lane: 'interview',
        slot: 1,
        of_turn: 3,
        at: 't/4',
        why: 'cargo test is running: the trunk is idle for a while',
        question: 'What did you just change? Answer with FACT: lines, citing files and lines.',
        prefix_tokens: 2306,
      },
      { kind: 'request', t: 11010, id: 'i/4/q', lane: 'interview', slot: 1, turn: 3, fork: 'i/4' },
      {
        kind: 'response',
        t: 13200,
        id: 'i/4/q#response',
        to_request: 'i/4/q',
        text: 'FACT: --json added to Args (src/cli.rs:31), conflicting with no_header\nFACT: Report::print writes JSON Lines when args.json (src/report.rs:43-51)',
        stop: 'stop',
        timings: timings(24, 2306, 30, 61, 1950),
      },
      { kind: 'fork.settled', t: 13210, id: 'i/4', outcome: 'complete' },
      { kind: 'patch', t: 13220, id: 'p/11', from: 'i/4', op: 'add', entry: { id: 'f4', category: 'Facts', text: '--json added to Args (src/cli.rs:31), conflicting with no_header' } },
      { kind: 'patch', t: 13220, id: 'p/12', from: 'i/4', op: 'add', entry: { id: 'f5', category: 'Facts', text: 'Report::print writes JSON Lines when args.json (src/report.rs:43-51)' } },
      {
        kind: 'tool.end',
        t: 25100,
        id: 't/5',
        exit: 0,
        output: '   Compiling tally v0.3.1 (/work/tally)\n    Finished `test` profile [unoptimized + debuginfo] target(s) in 13.62s\n     Running unittests src/main.rs\n\nrunning 9 tests\n.........\ntest result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s',
      },
      { kind: 'request', t: 25120, id: 'q/8', lane: 'trunk', slot: 0, turn: 3 },
      {
        kind: 'response',
        t: 34800,
        id: 'q/8#response',
        to_request: 'q/8',
        reasoning: 'All nine pass, including the two new cases. Summarise with an example.',
        text: 'Done.\n\n- `tally --json` prints one JSON object per file and a final `{"total": …}` line.\n- Field names are the table headers, lower-cased.\n- Two new tests, one per shape; all 9 pass.\n\n```\n$ tally --json README.md src/main.rs\n{"file":"README.md","words":412,"lines":61,"bytes":2710}\n{"file":"src/main.rs","words":131,"lines":48,"bytes":1204}\n{"total":{"words":543,"lines":109,"bytes":3914}}\n```',
        stop: 'stop',
        timings: timings(182, 2324, 140, 318, 9400),
      },
      { kind: 'turn.settled', t: 34820, turn: 3, reason: 'final' },
    ],
  },
];

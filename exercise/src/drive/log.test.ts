import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import { fold } from '../session/fold.ts';
import type { LogLine } from './log.ts';
import { PHASE_PROPOSED, PHASE_RULED_CONTINUE } from './served/phase-proposal.ts';
import { SELF_CAPTURE } from './served/self-capture.ts';
import { TANGENT_CLOSED, TANGENT_OPEN } from './served/tangent.ts';
import { needsOf } from './log.ts';

/**
 * `diet`'s own valid logs (`diet/formats/log/fixtures/valid/`), folded. A
 * line's shape is `diet`'s -- its types generated from the format (#144) and
 * checked in its CI -- so what is left to hold here is the surface's half:
 * every fixture folds, into a session, with no line the fold counts as
 * unknown. A kind `diet` gains fails here before it fails in a session.
 *
 * The events are READ BY `diet`: each fixture's `.expected.json` is what
 * `diet check-log` projects from it, pinned by the conformance corpus. A
 * second reader here, splitting the raw text, would need a second rule for
 * a torn final line (#230), which `diet`'s reader sets aside and counts.
 */
const valid = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../../diet/formats/log/fixtures/valid');
const fixtures = readdirSync(valid).filter((f) => f.endsWith('.jsonl')).sort();

const logOf = (file: string): LogLine[] =>
  (
    JSON.parse(readFileSync(path.join(valid, file.replace(/\.jsonl$/, '.expected.json')), 'utf8')) as {
      events: LogLine[];
    }
  ).events;

describe("diet's valid v0 logs, as the surface reads them", () => {
  it('finds the fixtures', () => {
    expect(fixtures.length).toBeGreaterThan(0);
  });

  for (const file of fixtures) {
    it(file, () => {
      const session = fold(logOf(file));
      expect([...session.unknown.keys()], `${file}: kinds the fold does not know`).toEqual([]);
      expect(session.state).not.toBe('connecting');
    });
  }

  it('folds an answered turn into an ask and its answer', () => {
    const session = fold(logOf('an-answered-turn.jsonl'));
    const nodes = session.eras[0]?.nodes ?? [];
    expect(nodes.map((n) => n.kind)).toEqual(['user', 'assistant']);
    const answer = nodes[1];
    const response = logOf('an-answered-turn.jsonl').find((l) => l.kind === 'response');
    expect(answer?.kind === 'assistant' && answer.progress).toBe('done');
    expect(answer?.kind === 'assistant' && answer.text).toBe(response?.kind === 'response' ? response.text : undefined);
  });

  it("carries the operator's edit and flag onto working memory (v7, #150)", () => {
    const file = 'a-v7-operator-edit-and-flag-carried-by-a-seam.jsonl';
    const memory = fold(logOf(file)).memory;
    const edited = memory.find((entry) => entry.id === 'd2');
    expect(edited?.lane).toBe('operator');
    expect(edited?.flag).toBe('is one team enough?');
    expect(memory.find((entry) => entry.id === 'd1')?.state).toBe('superseded');
  });

  it('carries a prune on the turn that made it (v7, #612)', () => {
    const file = 'a-v7-prune-applied-at-the-turns-seam.jsonl';
    const line = logOf(file).find((l) => l.kind === 'pruned');
    const user = fold(logOf(file))
      .eras.flatMap((era) => era.nodes)
      .find((n) => n.kind === 'user');
    expect(line?.kind).toBe('pruned');
    expect(user?.kind === 'user' && user.pruned).toEqual(
      line?.kind === 'pruned' ? [{ call: line.call, bytes: line.bytes, text: line.text }] : undefined,
    );
  });

  it("opens a second era at a served seam (v6, #493): its system prompt is the head's and the render, and it says what it carried", () => {
    const file = 'a-v6-seam-the-operator-declared.jsonl';
    const line = logOf(file).find((l) => l.kind === 'seam');
    const session = fold(logOf(file));
    expect(session.eras).toHaveLength(2);
    const era = session.eras[1]!;
    const head = logOf(file).find((l) => l.kind === 'session.start');
    const system = head?.kind === 'session.start' ? head.head.find((m) => m.role === 'system')?.content : undefined;
    // The head's system message, a blank line, then the render: what `refill` sends.
    expect(era.system.text).toBe(line?.kind === 'seam' ? `${system}\n\n${line.render}` : undefined);
    expect(era.system.render).toBe('placeholder');
    expect(era.seam?.carried).toEqual({ entries: 1, turns: 0 });
    expect(era.nodes.map((n) => n.kind)).toEqual(['user', 'assistant']);
  });

  it('a seam since #597 leaves the system prompt alone: the era opens on the user message carrying the render', () => {
    const file = 'a-v7-seam-whose-refill-is-a-message.jsonl';
    const line = logOf(file).find((l) => l.kind === 'seam');
    const era = fold(logOf(file)).eras[1]!;
    expect(era.system.placement).toBe('message');
    expect(era.system.text).toBe(line?.kind === 'seam' ? `<summary>\n${line.render}\n</summary>` : undefined);
  });

  it('folds a stopped call as cancelled, keeping what arrived', () => {
    const answer = fold(logOf('a-cancelled-turn.jsonl')).eras[0]?.nodes.find((n) => n.kind === 'assistant');
    expect(answer?.kind === 'assistant' && answer.progress).toBe('cancelled');
    expect(answer?.kind === 'assistant' && answer.text).toBe('Hel');
  });

  it('keeps a cancelled turn that had said something in the model’s context, as diet now does (#575), and marks one cancelled before a word out', () => {
    const marks = (log: LogLine[]) => (fold(log).eras[0]?.nodes ?? []).filter((n) => n.kind === 'user' || n.kind === 'assistant').map((n) => [n.kind, 'outOfContext' in n ? n.outOfContext : false]);
    const said = logOf('a-cancelled-turn.jsonl');
    expect(marks(said)).toEqual([['user', false], ['assistant', false]]);
    const silent = said.map((line) => (line.kind === 'cancelled' ? { ...line, partial: '' } : line)) as LogLine[];
    expect(marks(silent)).toEqual([['user', 'cancelled'], ['assistant', 'cancelled']]);
  });

  it('marks a failed or timed-out turn’s ask and answer as out of the model’s context, by its settle word, and an answered one’s not (#289)', () => {
    const marks = (file: string) => (fold(logOf(file)).eras[0]?.nodes ?? []).filter((n) => n.kind === 'user' || n.kind === 'assistant').map((n) => [n.kind, 'outOfContext' in n ? n.outOfContext : false]);
    expect(marks('a-turn-whose-connection-failed.jsonl')).toEqual([['user', 'failed'], ['assistant', 'failed']]);
    expect(marks('a-turn-that-ran-out-of-time.jsonl')).toEqual([['user', 'timeout'], ['assistant', 'timeout']]);
    expect(marks('an-answered-turn.jsonl')).toEqual([['user', false], ['assistant', false]]);
  });

  it('keeps a turn that failed after its tool steps on the trunk, as diet now does (#541): only its failing step is out of context', () => {
    // diet's own turn of one call that ran, then a second request that failed: the step a later request followed stays.
    const ran = logOf('a-v3-tool-call-that-ran.jsonl');
    const at = ran.length;
    const failedAfter: LogLine[] = [
      ...ran,
      { kind: 'request', lane: 'trunk', seq: at, t: 95, turn: 1 },
      { kind: 'request.failed', reason: 'transport', message: 'could not connect: refused', request: at, seq: at + 1, t: 100 },
      { kind: 'turn.settled', reason: 'failed', seq: at + 2, t: 105, turn: 1 },
    ] as LogLine[];
    const marks = (log: LogLine[]) => (fold(log).eras[0]?.nodes ?? []).filter((n) => n.kind === 'user' || n.kind === 'assistant').map((n) => [n.kind, 'outOfContext' in n ? n.outOfContext : false]);
    expect(marks(failedAfter)).toEqual([
      ['user', false],
      ['assistant', false],
      ['assistant', 'failed'],
    ]);
    // A turn that failed on its first request ran nothing and keeps nothing: ask and answer both out, as before.
    expect(marks(logOf('a-turn-whose-connection-failed.jsonl'))).toEqual([['user', 'failed'], ['assistant', 'failed']]);
  });

  it('folds a reasoning delta as reasoning, not answer', () => {
    const log = logOf('a-reasoning-delta.jsonl');
    const answer = fold(log).eras[0]?.nodes.find((n) => n.kind === 'assistant');
    expect(answer?.kind === 'assistant' && answer.reasoning).toBe('thinking\nstill');
    expect(answer?.kind === 'assistant' && answer.text).toBe('ok');
  });

  it("reads a failure by its reason, never its message: two failures differing only in message fold alike (ruled on #140)", () => {
    const log = logOf('a-turn-whose-connection-failed.jsonl');
    const withMessage = (message: string) => fold(log.map((l) => (l.kind === 'request.failed' ? { ...l, message } : l)));
    const [bare, told] = [withMessage(''), withMessage('The server did not answer within 5s. It may be loading a model, out of memory, or gone; the transport gave up waiting and dropped the connection after one attempt.')];
    // The message is shown, verbatim, and read for nothing: take it out and the two sessions are the same.
    const unsaid = (session: typeof bare) =>
      JSON.stringify({
        state: session.state,
        receipt: session.receipt,
        eras: session.eras.map((era) => era.nodes.map((n) => (n.kind === 'assistant' && n.failure ? { ...n, failure: { ...n.failure, message: '' } } : n))),
      });
    expect(log.some((l) => l.kind === 'request.failed')).toBe(true);
    expect(unsaid(told)).toBe(unsaid(bare));
    const failed = told.eras[0]?.nodes.find((n) => n.kind === 'assistant');
    expect(failed?.kind === 'assistant' && failed.failure?.reason).toBe('transport');
  });

  it("draws diet's forks off the trunk: a fork line names no slot, so each goes to a side slot, and the session has one (v5)", () => {
    const file = 'a-v5-scoping-fork-that-patched-and-a-read-fork-that-declined.jsonl';
    const session = fold(logOf(file));
    const branches = [...session.branches.values()].flat();
    expect(branches).toHaveLength(logOf(file).filter((l) => l.kind === 'fork').length);
    for (const b of branches) {
      expect(b.slot).not.toBe(session.trunkSlot);
      expect(b.slot).toBeLessThan(session.slots);
    }
  });

  it('needs nothing ahead of the format on any line diet writes: the gaps overlay outlines none of it (#503)', () => {
    for (const file of fixtures) for (const line of logOf(file)) expect(needsOf(line), `${file}: seq ${line.seq} (${line.kind})`).toEqual([]);
  });

  it('reads the lever states a session declares off its first line, and leaves out what it does not declare (#573)', () => {
    const levers = (file: string) => fold(logOf(file)).levers;
    expect(levers('a-v7-call-that-ran-with-approvals-off.jsonl')).toEqual({ approvals: 'off' });
    expect(levers('a-v7-imperative-delivery-after-an-ask.jsonl')).toEqual({ approvals: 'gate', forkDelivery: 'imperative' });
    expect(levers('a-v7-session-sending-thinking-off.jsonl')).toEqual({ approvals: 'gate', reasoning: 'thinking off' });
    expect(levers('a-v7-session-sending-a-reasoning-effort.jsonl')).toEqual({ approvals: 'gate', reasoning: 'effort medium' });
    // Before v7 the log has no approval lever to declare: undeclared, not assumed.
    expect(levers('an-answered-turn.jsonl')).toEqual({});
  });

  it('folds a tangent: open, its entries are its own; closed, they are ruled on and its turns rolled back (#608)', () => {
    const open = fold([...TANGENT_OPEN]);
    expect(open.tangent).toEqual({ id: 't/1', entries: ['e1', 'e2'] });
    expect(open.tangentsOpened).toBe(1);
    const closed = fold([...TANGENT_CLOSED]);
    expect(closed.tangent).toBeUndefined();
    expect(closed.memory.map((e) => [e.id, e.state])).toEqual([
      ['e1', 'live'],
      ['e2', 'retired'],
    ]);
    // The turn asked inside the tangent left the trunk at its close; the one before it did not.
    const marks = (closed.eras[0]?.nodes ?? []).filter((n) => n.kind === 'user' || n.kind === 'assistant').map((n) => [n.kind, n.turn, 'outOfContext' in n ? n.outOfContext : false]);
    expect(marks).toEqual([
      ['user', 1, false],
      ['assistant', 1, false],
      ['user', 2, 'rolled-back'],
      ['assistant', 2, 'rolled-back'],
    ]);
  });

  it('reads the phase graph off the first line, opens in its opening phase, and moves with each seam that moved (#563)', () => {
    const log = logOf('a-v7-seam-that-moved-a-phase.jsonl');
    const opened = fold(log.slice(0, 1));
    expect(opened.phase).toBe('plan');
    expect(opened.phaseMoves).toEqual(['build']);
    const moved = fold(log);
    expect(moved.phase).toBe('build');
    expect(moved.phaseMoves).toEqual(['review']);
    // A log with no graph offers no move.
    expect(fold(logOf('an-answered-turn.jsonl')).phaseMoves).toEqual([]);
  });

  it('folds the harness’s notes onto the ask they followed, and a self-capture call’s outcome onto the call (#574)', () => {
    const userOf = (log: LogLine[], turn: number) => fold(log).eras[0]?.nodes.find((n) => n.kind === 'user' && n.turn === turn);
    const delivered = userOf(logOf('a-v7-imperative-delivery-after-an-ask.jsonl'), 2);
    expect(delivered?.kind === 'user' && delivered.delivered?.framing).toBe('imperative');
    const recalled = userOf(logOf('a-v7-recall-after-an-ask.jsonl'), 2);
    expect(recalled?.kind === 'user' && recalled.recalled?.recall).toBe('literal');
    const session = fold([...SELF_CAPTURE]);
    expect([...session.unknown.keys()]).toEqual([]);
    const reminded = session.eras[0]?.nodes.find((n) => n.kind === 'user' && n.turn === 2);
    expect(reminded?.kind === 'user' && reminded.reminded).toBe('If this turn settled anything worth keeping, record it with update_record.');
    const call = session.eras[0]?.nodes.find((n) => n.kind === 'tool');
    expect(call?.kind === 'tool' && call.capture).toEqual({ outcome: 'recorded', entries: ['r10/c1/fact'] });
  });

  it('keeps the whole lever table session.start declares, as given, and none for a log from before it (#573, #623)', () => {
    const declared = logOf('a-v7-session-declaring-its-levers.jsonl');
    const start = declared[0];
    expect(fold(declared).levers.table).toEqual(start?.kind === 'session.start' ? start.levers : undefined);
    expect(fold(declared).levers.table).not.toBeUndefined();
    expect(fold(logOf('an-answered-turn.jsonl')).levers.table).toBeUndefined();
  });

  it('folds an offboard fork’s seat onto its branch, with its prefill and wall time, and a warm fork’s as before (#570, #615)', () => {
    // diet's v5 forks, the first one moved offboard -- WRITTEN HERE: no diet fixture has an offboard fork yet.
    const file = 'a-v5-scoping-fork-that-patched-and-a-read-fork-that-declined.jsonl';
    const log = logOf(file);
    const first = log.find((l) => l.kind === 'fork')!;
    const offboard = log.map((l) =>
      l === first ? { ...l, substrate: 'mac-pro-llamacpp-qwen3-4b', model: 'qwen3-4b' } : l.kind === 'fork.settled' && l.fork === first.seq ? { ...l, prompt_tokens: 1820, wall_ms: 4300 } : l,
    ) as LogLine[];
    const branches = [...fold(offboard).branches.values()].flat();
    expect(branches.map((b) => b.seat)).toEqual([{ substrate: 'mac-pro-llamacpp-qwen3-4b', model: 'qwen3-4b', promptTokens: 1820, wallMs: 4300 }, undefined]);
  });

  it('draws the trunk’s own working-memory changes from their patch lines, each with its lane (#574, #627)', () => {
    const session = fold(logOf('a-v7-self-capture-patch-named-by-its-lane.jsonl'));
    expect([...session.unknown.keys()]).toEqual([]);
    expect(session.memory.map((e) => [e.id, e.text, e.state, e.lane])).toEqual([['r3/call-1/fact', 'The parser drops blank lines before it tokenizes.', 'live', 'self-capture']]);
  });

  it('folds an overflow with its sizes and who told it, a window seam with its size, a pruned call’s replacement, and a fork refused for the pool (#628, #633, #630, #637)', () => {
    const answerOf = (file: string) => fold(logOf(file)).eras.flatMap((e) => e.nodes).find((n) => n.kind === 'assistant');
    const inferred = answerOf('a-v7-overflow-told-from-the-prompts-size.jsonl');
    expect(inferred?.kind === 'assistant' && inferred.failure?.overflow).toEqual({ promptTokens: 161840, window: 163840, inferred: true });
    const reported = answerOf('a-context-overflow.jsonl');
    expect(reported?.kind === 'assistant' && reported.failure?.reason).toBe('context_overflow');
    expect(reported?.kind === 'assistant' && reported.failure?.overflow).toBeUndefined();
    const windowed = fold(logOf('a-v7-window-seam-under-the-turn-it-makes-fit.jsonl'));
    expect(windowed.eras[1]?.seam).toMatchObject({ reason: 'window', size: { promptTokens: 150000, window: 163840 } });
    const pruned = fold(logOf('a-v7-prune-applied-at-the-turns-seam.jsonl')).eras[0]?.nodes.find((n) => n.kind === 'tool' && n.tool === 'bash');
    expect(pruned?.kind === 'tool' && pruned.pruned).toEqual({ bytes: 14, replaced: true });
    const [refused] = [...fold(logOf('a-v7-fork-refused-for-the-pool-and-one-that-may-displace-the-trunk-cache.jsonl')).branches.values()].flat();
    expect([refused?.outcome, refused?.refused, refused?.hazard]).toEqual(['refused', 'pool', 'may-displace-trunk-cache']);
  });

  it('folds a call moved to the background with its job and how it ended, the harness’s notice, and a fork’s trigger (#614, #620)', () => {
    const session = fold(logOf('a-v7-call-started-in-the-background.jsonl'));
    expect([...session.unknown.keys()]).toEqual([]);
    const calls = session.eras[0]?.nodes.filter((n) => n.kind === 'tool') ?? [];
    expect(calls.map((c) => c.kind === 'tool' && c.background)).toEqual([
      { job: 'bg_0123abcd', status: 'completed', exit: 0 },
      { job: 'bg_4567cdef', status: 'cancelled' },
    ]);
    const asked = session.eras[0]?.nodes.find((n) => n.kind === 'user' && n.turn === 2);
    expect(asked?.kind === 'user' && asked.noticed).toBe('<task-notification>\n<task-id>bg_0123abcd</task-id>\n</task-notification>');
    const triggers = [...fold(logOf('a-v7-gap-with-two-triggered-forks.jsonl')).branches.values()].flat().map((b) => b.trigger);
    expect(triggers).toEqual(['call:document-read:c1', 'turn_end']);
  });

  it('folds a seam’s audit on its own lane: the entries it kept, updated and removed (#646)', () => {
    const [audit] = [...fold(logOf('a-v7-seam-audited-on-its-lane-and-warmed.jsonl')).branches.values()].flat().filter((b) => b.lane === 'audit');
    expect(audit?.outcome).toBe('value');
    expect(audit?.audit).toEqual({ kept: [], updated: ['d1'], removed: [] });
  });

  it('folds the model’s phase proposal as pending until the operator rules on it, and a “continue” moves the phase (#124, #651)', () => {
    const pending = fold([...PHASE_PROPOSED]);
    expect(pending.proposal).toEqual({ call: 'p1', from: 'plan', to: 'build', reason: 'the spec is settled' });
    expect(pending.phase).toBe('plan');
    const ruled = fold([...PHASE_RULED_CONTINUE]);
    expect([...ruled.unknown.keys()]).toEqual([]);
    expect(ruled.proposal).toBeUndefined();
    expect(ruled.phase).toBe('build');
    const call = ruled.eras[0]?.nodes.find((n) => n.kind === 'tool' && n.tool === 'propose_phase_transition');
    expect(call?.kind === 'tool' && call.ruled).toEqual({ choice: 'continue', to: 'build' });
  });

  it('folds the operator’s edit and flag, the model writes refused over that entry, and the seam’s account of them (#150, #655)', () => {
    const session = fold(logOf('a-v7-operator-edit-and-flag-carried-by-a-seam.jsonl'));
    expect([...session.unknown.keys()]).toEqual([]);
    const d2 = session.memory.find((e) => e.id === 'd2');
    expect([d2?.lane, d2?.flag, d2?.refusedWrites]).toEqual([
      'operator',
      'is one team enough?',
      [
        { op: 'retire', by: 'self-capture' },
        { op: 'supersede', by: 'fork 7' },
      ],
    ]);
    expect(session.eras[1]?.seam).toMatchObject({ operatorChanges: [{ entry: 'd2', kind: 'edit' }, { entry: 'd2', kind: 'flag' }], unaddressed: ['d2'] });
  });

  it('takes the state from the log: an ended session is ended', () => {
    expect(fold(logOf('an-ended-session.jsonl')).state).toBe('ended');
  });
});

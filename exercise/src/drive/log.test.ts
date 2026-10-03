import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import { fold } from '../session/fold.ts';
import type { LogLine } from './log.ts';

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

  it('folds a stopped call as cancelled, keeping what arrived', () => {
    const answer = fold(logOf('a-cancelled-turn.jsonl')).eras[0]?.nodes.find((n) => n.kind === 'assistant');
    expect(answer?.kind === 'assistant' && answer.progress).toBe('cancelled');
    expect(answer?.kind === 'assistant' && answer.text).toBe('Hel');
  });

  it('marks a cancelled turn’s ask and answer as out of the model’s context, and an answered one’s not (#289)', () => {
    const marks = (file: string) => (fold(logOf(file)).eras[0]?.nodes ?? []).filter((n) => n.kind === 'user' || n.kind === 'assistant').map((n) => [n.kind, 'outOfContext' in n && n.outOfContext === true]);
    expect(marks('a-cancelled-turn.jsonl')).toEqual([['user', true], ['assistant', true]]);
    expect(marks('an-answered-turn.jsonl')).toEqual([['user', false], ['assistant', false]]);
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

  it('takes the state from the log: an ended session is ended', () => {
    expect(fold(logOf('an-ended-session.jsonl')).state).toBe('ended');
  });
});

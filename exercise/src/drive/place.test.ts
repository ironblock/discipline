import { describe, expect, it } from 'vitest';

import { compose } from './compose.ts';
import { KITCHEN_SINK_SCRIPT } from './kitchen-sink.ts';
import type { LineOf, LogLine } from './log.ts';
import { place, Placer } from './place.ts';
import type { Unplaced } from './script.ts';
import { fold } from '../session/fold.ts';

/** A placed log is served-shaped (ruled on #300, 5974672622): one order, the one a server writes. */
describe('a placed tool call', () => {
  const script = compose(KITCHEN_SINK_SCRIPT);
  const { log, labels } = place(script);
  const fragments = log.filter((l): l is Extract<LineOf<'delta'>, { tool_call: unknown }> => l.kind === 'delta' && 'tool_call' in l && l.tool_call !== undefined);
  const seqOf = (kind: LogLine['kind'], test: (l: LogLine) => boolean) => log.find((l) => l.kind === kind && test(l))?.seq ?? Number.NaN;

  it('says it was placed, not served, on its first line (#297 5976392264)', () => {
    expect(log[0]).toMatchObject({ kind: 'session.start', version: 3, provenance: 'placed' });
  });

  it('streams before its response, as a server writes it', () => {
    expect(fragments.length).toBeGreaterThan(0);
    for (const f of fragments) expect(f.seq).toBeLessThan(seqOf('response', (l) => l.kind === 'response' && l.to_request === f.request));
  });

  it('leaves one outcome line per call, after its response, naming the call as streamed', () => {
    const lines = log.filter((l) => l.kind === 'tool_call');
    expect(lines.map((l) => l.id).sort()).toEqual(fragments.map((f) => f.tool_call.id).sort());
    for (const l of lines) expect(l.seq).toBeGreaterThan(seqOf('response', (r) => r.kind === 'response' && r.to_request === l.request));
  });

  it('says a bash call’s confinement was not recorded, and a call of another tool carries its arguments only', () => {
    const lines = log.filter((l) => l.kind === 'tool_call');
    const bash = lines.filter((l) => l.name === 'bash');
    expect(bash.length).toBeGreaterThan(0);
    for (const l of bash) expect([l.argv?.slice(0, 2), l.isolation, l.network, 'confined' in l]).toEqual([['sh', '-c'], 'unrecorded', 'unrecorded', false]);
    const read = new Placer();
    const head: Unplaced[] = [
      { kind: 'session.start', t: 0, model: 'm', arm: 'a', slots: 1, trunk_slot: 0, phase: 'p', system: { text: 's' } },
      { kind: 'ask', t: 1, turn: 1, text: 'go' },
      { kind: 'request', t: 2, id: 'q/1', lane: 'trunk', slot: 0, turn: 1 },
      { kind: 'response', t: 3, id: 'q/1#response', to_request: 'q/1', text: '', stop: 'tool', timings: { prompt_n: 1, cache_n: 0, prompt_ms: 1, predicted_n: 1, predicted_ms: 1 } },
      { kind: 'tool.begin', t: 4, id: 't/1', turn: 1, after: 'q/1#response', tool: 'read', args: { path: 'a.txt' } },
    ];
    head.forEach((e, i) => read.place(e, head.slice(i + 1)));
    const [line] = read.place({ kind: 'tool.end', t: 5, id: 't/1', exit: 0, output: 'é' });
    expect(Object.keys(line ?? {}).sort()).toEqual(['arguments', 'exit', 'id', 'kind', 'name', 'outcome', 'request', 'seq', 'stderr', 'stderr_bytes', 'stdout', 'stdout_bytes', 't', 'turn']);
    // A stream's byte count is its UTF-8's (2 for `é`), not its length in UTF-16 units (1).
    expect(line).toMatchObject({ stdout: 'é', stdout_bytes: 2, stderr: '', stderr_bytes: 0 });
  });

  it('is what a fork naming the call branches from: its first fragment', () => {
    const forksAtCalls = script.filter((e): e is Extract<Unplaced, { kind: 'fork' }> => e.kind === 'fork' && e.at.startsWith('t/'));
    expect(forksAtCalls.length).toBeGreaterThan(0);
    for (const fork of forksAtCalls) {
      const fragment = fragments.find((f) => f.tool_call.id === fork.at);
      expect(labels.get(fork.at)).toBe(fragment?.seq);
      expect(log.find((l) => l.kind === 'fork' && l.seq === labels.get(fork.id))).toMatchObject({ at: fragment?.seq });
    }
  });

  it('counts a response’s calls, and the fold draws each, the second begun only when the first has ended', () => {
    const placer = new Placer();
    const script: Unplaced[] = [
      { kind: 'session.start', t: 0, model: 'm', arm: 'a', slots: 1, trunk_slot: 0, phase: 'p', system: { text: 's' } },
      { kind: 'ask', t: 1, turn: 1, text: 'go' },
      { kind: 'request', t: 2, id: 'q/1', lane: 'trunk', slot: 0, turn: 1 },
      { kind: 'response', t: 10, id: 'q/1#response', to_request: 'q/1', text: '', stop: 'tool', timings: { prompt_n: 1, cache_n: 0, prompt_ms: 1, predicted_n: 1, predicted_ms: 1 } },
      { kind: 'tool.begin', t: 11, id: 't/1', turn: 1, after: 'q/1#response', tool: 'bash', args: { command: 'one' } },
      { kind: 'tool.end', t: 50, id: 't/1', exit: 0, output: '1' },
      { kind: 'tool.begin', t: 51, id: 't/2', turn: 1, after: 'q/1#response', tool: 'bash', args: { command: 'two' } },
      { kind: 'tool.end', t: 90, id: 't/2', exit: 0, output: '2' },
    ];
    const placed = script.flatMap((e, i) => placer.place(e, script.slice(i + 1)));
    const pieces = placed.flatMap((l) => (l.kind === 'delta' && 'tool_call' in l && l.tool_call ? [l.tool_call] : []));
    expect(pieces.map((p) => [p.index, p.id])).toEqual([
      [0, 't/1'],
      [1, 't/2'],
    ]);
    const tools = (lines: readonly LogLine[]) => fold(lines).eras.flatMap((era) => era.nodes).filter((n) => n.kind === 'tool');
    expect(tools(placed).map((n) => n.kind === 'tool' && [n.args, n.startedAt, n.ms])).toEqual([
      [{ command: 'one' }, 10, 40],
      [{ command: 'two' }, 50, 40],
    ]);
    // While the first runs, the second has not begun.
    const midway = placed.filter((l) => l.kind !== 'tool_call');
    expect(tools(midway).map((n) => n.kind === 'tool' && [n.running, n.waiting])).toEqual([
      [true, undefined],
      [false, true],
    ]);
  });

  it('cut off mid-command, is cancelled: no exit or output, and a bash call’s argv and unrecorded words, never confined', () => {
    const placer = new Placer();
    const head: Unplaced[] = [
      { kind: 'session.start', t: 0, model: 'm', arm: 'a', slots: 1, trunk_slot: 0, phase: 'p', system: { text: 's' } },
      { kind: 'ask', t: 1, turn: 1, text: 'go' },
      { kind: 'request', t: 2, id: 'q/1', lane: 'trunk', slot: 0, turn: 1 },
      { kind: 'response', t: 3, id: 'q/1#response', to_request: 'q/1', text: '', stop: 'tool', timings: { prompt_n: 1, cache_n: 0, prompt_ms: 1, predicted_n: 1, predicted_ms: 1 } },
      { kind: 'tool.begin', t: 4, id: 't/1', turn: 1, after: 'q/1#response', tool: 'bash', args: { command: 'sleep 9' } },
    ];
    const placed = head.flatMap((e, i) => placer.place(e, head.slice(i + 1)));
    const [line] = placer.place({ kind: 'tool.end', t: 5, id: 't/1', exit: 0, output: '', cancelled: true });
    expect(placed.filter((l) => l.kind === 'delta').map((l) => l.seq)).toEqual([3]);
    expect(line).toEqual({
      seq: 5,
      kind: 'tool_call',
      t: 5,
      request: 2,
      turn: 1,
      id: 't/1',
      name: 'bash',
      arguments: '{"command":"sleep 9"}',
      outcome: 'cancelled',
      argv: ['sh', '-c', 'sleep 9'],
      isolation: 'unrecorded',
      network: 'unrecorded',
    });
  });
});

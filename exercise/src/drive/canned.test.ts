import { describe, expect, it } from 'vitest';

import { fold } from '../session/fold.ts';
import { CannedTransport } from './canned.ts';
import type { LogLine } from './log.ts';
import { SPECIMEN } from './specimen.ts';

/** The canned drive, one turn in and settled: the gap its settling opened is the one a command may carry. */
async function settled() {
  const transport = new CannedTransport(SPECIMEN, { speed: 1_000_000 });
  const lines: LogLine[] = [];
  transport.subscribe((line) => lines.push(line));
  await transport.dispatch({ kind: 'ask', text: 'first' });
  await new Promise((resolve) => setTimeout(resolve, 200));
  const open = lines.findLast((l) => l.kind === 'turn.settled');
  if (!open) throw new Error('the first turn did not settle');
  return { transport, lines, open: open.seq };
}

const gap = (opened_by: number, ended_by: 'ask' | 'seam' | 'cancel' | 'end' = 'ask') => ({ opened_by, notice: 0, read: 1000, compose: 500, away: 0, blocked: 0, ended_by });

describe('the canned transport, as the drive (#146, ruling (b) on #117)', () => {
  it('logs an admitted command’s gap just before the first line the command pushes', async () => {
    const { transport, lines, open } = await settled();
    const before = lines.length;
    await expect(transport.dispatch({ kind: 'ask', text: 'next' }, { idle_gap: gap(open) })).resolves.toEqual({ ok: true });
    transport.close();
    expect(lines[before]).toMatchObject({ ...gap(open), kind: 'idle.gap', seq: before });
    expect(lines[before + 1]?.kind).toBe('ask');
    expect(lines.every((l, i) => l.seq === i)).toBe(true);
  });

  it('does not log a gap that is not the open one, or is ended by another kind of command -- and the command goes ahead without it', async () => {
    const { transport, lines, open } = await settled();
    const before = lines.length;
    await expect(transport.dispatch({ kind: 'ask', text: 'next' }, { idle_gap: gap(open + 1) })).resolves.toEqual({ ok: true });
    transport.close();
    expect(lines[before]?.kind).toBe('ask');
    expect(lines.slice(before).some((l) => l.kind === 'idle.gap')).toBe(false);
    const other = await settled();
    await expect(other.transport.dispatch({ kind: 'ask', text: 'next' }, { idle_gap: gap(other.open, 'seam') })).resolves.toEqual({ ok: true });
    other.transport.close();
    expect(other.lines.some((l) => l.kind === 'idle.gap')).toBe(false);
  });

  it('opens a gap at a cancel’s settling, which the next command carries', async () => {
    const { transport, lines, open } = await settled();
    await expect(transport.dispatch({ kind: 'ask', text: 'next' }, { idle_gap: gap(open) })).resolves.toEqual({ ok: true });
    await expect(transport.dispatch({ kind: 'cancel' })).resolves.toEqual({ ok: true });
    const cancelled = lines.findLast((l) => l.kind === 'turn.settled');
    expect(cancelled).toMatchObject({ reason: 'cancelled' });
    const before = lines.length;
    // The script's next beat is the refill.
    await expect(transport.dispatch({ kind: 'seam', to: 'build' }, { idle_gap: gap(cancelled?.seq ?? -1, 'seam') })).resolves.toEqual({ ok: true });
    transport.close();
    expect(lines[before]).toMatchObject({ kind: 'idle.gap', opened_by: cancelled?.seq, ended_by: 'seam' });
  });

  it('drops the gap of a refused command', async () => {
    const { transport, lines, open } = await settled();
    const before = lines.length;
    // The script expects an ask next, not a refill: the seam is refused, and its gap with it.
    const ack = await transport.dispatch({ kind: 'seam', to: 'build' }, { idle_gap: gap(open, 'seam') });
    transport.close();
    expect(ack.ok).toBe(false);
    expect(lines.slice(before).some((l) => l.kind === 'idle.gap')).toBe(false);
  });
});

describe('the canned transport, ended (#289, as `diet`’s `Session::end`)', () => {
  it('ends while awaiting: the gap it carries, then the settlement into ended, and the session is ended', async () => {
    const { transport, lines, open } = await settled();
    const before = lines.length;
    await expect(transport.dispatch({ kind: 'end' }, { idle_gap: gap(open, 'end') })).resolves.toEqual({ ok: true });
    transport.close();
    expect(lines.slice(before).map((l) => l.kind)).toEqual(['idle.gap', 'settlement']);
    expect(lines.at(-1)).toMatchObject({ kind: 'settlement', from: 'awaiting', to: 'ended' });
    expect(fold(lines).state).toBe('ended');
  });

  it('refuses while work is in flight, and once ended', async () => {
    const { transport } = await settled();
    await transport.dispatch({ kind: 'ask', text: 'next' });
    await expect(transport.dispatch({ kind: 'end' })).resolves.toEqual({ ok: false, refused: 'in-flight' });
    await transport.dispatch({ kind: 'cancel' });
    await expect(transport.dispatch({ kind: 'end' })).resolves.toEqual({ ok: true });
    await expect(transport.dispatch({ kind: 'end' })).resolves.toEqual({ ok: false, refused: 'ended' });
    transport.close();
  });
});

describe('the canned transport, cancelled mid-call (#300)', () => {
  it('ends a call whose fragment streamed with its response, before its begin played: every call leaves one line', async () => {
    const transport = new CannedTransport(SPECIMEN, { speed: 1 });
    const lines: LogLine[] = [];
    let cancelled = false;
    transport.subscribe((line) => {
      lines.push(line);
      // Just after a response that ends in calls: its fragments are placed, its `tool.begin` is still a timer.
      if (!cancelled && line.kind === 'response' && line.finish_reason === 'tool_calls') {
        cancelled = true;
        queueMicrotask(() => void transport.dispatch({ kind: 'cancel' }));
      }
    });
    await transport.dispatch({ kind: 'ask', text: 'first' });
    for (let i = 0; i < 400 && !lines.some((l) => l.kind === 'turn.settled'); i++) await new Promise((resolve) => setTimeout(resolve, 25));
    transport.close();
    expect(cancelled).toBe(true);
    const ids = (kind: 'fragment' | 'line') =>
      lines.flatMap((l) => (kind === 'fragment' ? (l.kind === 'delta' && 'tool_call' in l && l.tool_call?.id !== undefined ? [l.tool_call.id] : []) : l.kind === 'tool_call' ? [l.id] : []));
    expect(ids('fragment').length).toBeGreaterThan(0);
    expect(ids('line').sort()).toEqual(ids('fragment').sort());
    expect(lines.filter((l) => l.kind === 'tool_call').map((l) => l.outcome)).toContain('cancelled');
    const tools = fold(lines).eras.flatMap((era) => era.nodes).filter((n) => n.kind === 'tool');
    expect(tools.some((n) => n.kind === 'tool' && n.running)).toBe(false);
  }, 15_000);
});

describe('the operator’s scope mark, canned (#453)', () => {
  it('places `scoping: true` on the marked ask’s line, and the fold draws its turn’s ask as the scope answer', async () => {
    const transport = new CannedTransport(SPECIMEN, { speed: 1_000_000 });
    const lines: LogLine[] = [];
    transport.subscribe((line) => lines.push(line));
    await transport.dispatch({ kind: 'ask', text: 'a json flag', scoping: true });
    transport.close();
    expect(lines.find((l) => l.kind === 'ask')).toMatchObject({ text: 'a json flag', scoping: true });
    const asked = fold(lines).eras.flatMap((e) => e.nodes).find((n) => n.kind === 'user');
    expect(asked?.kind === 'user' && asked.scoping).toBe(true);
  });

  it('places no `scoping` on an unmarked ask', async () => {
    const transport = new CannedTransport(SPECIMEN, { speed: 1_000_000 });
    const lines: LogLine[] = [];
    transport.subscribe((line) => lines.push(line));
    await transport.dispatch({ kind: 'ask', text: 'a json flag' });
    transport.close();
    expect(lines.find((l) => l.kind === 'ask')).not.toHaveProperty('scoping');
  });
});

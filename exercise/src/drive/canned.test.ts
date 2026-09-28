import { describe, expect, it } from 'vitest';

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

const gap = (opened_by: number, ended_by: 'ask' | 'seam' | 'cancel' = 'ask') => ({ opened_by, notice: 0, read: 1000, compose: 500, away: 0, blocked: 0, ended_by });

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

  it('turns away a gap that is not the open one, or is ended by another kind of command: nothing logged', async () => {
    const { transport, lines, open } = await settled();
    const before = lines.length;
    await expect(transport.dispatch({ kind: 'ask', text: 'next' }, { idle_gap: gap(open + 1) })).resolves.toEqual({ ok: false, refused: 'bad-gap' });
    await expect(transport.dispatch({ kind: 'ask', text: 'next' }, { idle_gap: gap(open, 'seam') })).resolves.toEqual({ ok: false, refused: 'bad-gap' });
    transport.close();
    expect(lines).toHaveLength(before);
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

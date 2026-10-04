import { describe, expect, it } from 'vitest';

import { fold } from '../session/fold.ts';
import type { Folded, ToolNode } from '../session/fold.ts';
import { APPROVAL, T1_CWD } from './approval.ts';
import { CannedTransport } from './canned.ts';
import type { LogLine } from './log.ts';
import type { Prompt } from './transport.ts';

/**
 * The canned drive holding a call on the operator (#389), as `serve` does
 * (#298 point 8): the rest of the turn waits on the answer; a scope plays it
 * on, the call carrying the decision (log v4's `approval`, #388); a decline
 * refuses it; a cancel cuts it off. And the replay of what it logged draws
 * the decision from the log alone.
 */
async function asked() {
  const transport = new CannedTransport(APPROVAL, { speed: 1_000_000 });
  const lines: LogLine[] = [];
  const prompts: (Prompt | undefined)[] = [];
  transport.subscribe((line) => lines.push(line));
  transport.watchPrompt((prompt) => prompts.push(prompt));
  await transport.dispatch({ kind: 'ask', text: 'Install the dependencies and start the dev server.' });
  await new Promise((resolve) => setTimeout(resolve, 50));
  return { transport, lines, prompts };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 50));
const calls = (lines: readonly LogLine[]) => lines.filter((l): l is Extract<LogLine, { kind: 'tool_call' }> => l.kind === 'tool_call');
const tools = (lines: readonly LogLine[]) => fold(lines).eras.flatMap((era) => era.nodes).filter((n): n is Folded<ToolNode> => n.kind === 'tool');

describe('a call waiting on the operator, canned (#389)', () => {
  it('holds the turn on the prompt: the call is drawn running, nothing after it is logged, and the session is busy', async () => {
    const { transport, lines, prompts } = await asked();
    expect(prompts.at(-1)).toMatchObject({ id: 't/1', command: 'npm install', cwd: T1_CWD, reason: 'not_approved' });
    expect(prompts.at(-1)?.request).toBe(lines.find((l) => l.kind === 'request')?.seq);
    expect(calls(lines)).toEqual([]);
    expect(lines.at(-1)?.kind).toBe('response');
    expect(tools(lines)[0]).toMatchObject({ running: true, call: { id: 't/1' } });
    await expect(transport.dispatch({ kind: 'ask', text: 'again' })).resolves.toEqual({ ok: false, refused: 'busy' });
    transport.close();
  });

  it('plays on when approved: the call ran under the scope, decided when the answer came, and why it was held', async () => {
    const { transport, lines, prompts } = await asked();
    await expect(transport.dispatch({ kind: 'approve', call: 't/1', scope: 'session' })).resolves.toEqual({ ok: true });
    expect(prompts.at(-1)).toBeUndefined();
    await settle();
    transport.close();
    const [ran, refused] = calls(lines);
    expect(ran).toMatchObject({ id: 't/1', outcome: 'ran', argv: ['sh', '-c', 'npm install'], cwd: T1_CWD, approval: { scope: 'session', why: 'not_approved' } });
    expect(ran?.approval?.decided_at).toBeGreaterThanOrEqual(lines.find((l) => l.kind === 'response')?.t ?? Infinity);
    expect(ran?.approval?.decided_at).toBeLessThanOrEqual(ran?.t ?? -1);
    // The denylist refuses without asking: the command it refused, its directory, and no policy words (#388 5982002587).
    expect(refused).toEqual(expect.objectContaining({ id: 't/2', outcome: 'refused', reason: 'denylist', argv: ['sh', '-c', 'rm -rf node_modules/.vite'], cwd: T1_CWD }));
    expect(refused).not.toHaveProperty('isolation');
    expect(refused).not.toHaveProperty('approval');
    expect(lines.at(-1)).toMatchObject({ kind: 'turn.settled', reason: 'final' });
    expect(lines.every((l, i) => l.seq === i)).toBe(true);
  });

  it('refuses the call when declined, and plays the script’s way on instead', async () => {
    const { transport, lines, prompts } = await asked();
    await expect(transport.dispatch({ kind: 'approve', call: 't/1', scope: 'decline' })).resolves.toEqual({ ok: true });
    expect(prompts.at(-1)).toBeUndefined();
    await settle();
    transport.close();
    expect(calls(lines)).toEqual([expect.objectContaining({ id: 't/1', outcome: 'refused', reason: 'declined', argv: ['sh', '-c', 'npm install'], cwd: T1_CWD })]);
    expect(calls(lines)[0]).not.toHaveProperty('approval');
    expect(lines.filter((l) => l.kind === 'response').at(-1)).toMatchObject({ text: expect.stringContaining('will not install') });
    expect(lines.at(-1)).toMatchObject({ kind: 'turn.settled', reason: 'final' });
  });

  it('turns away an answer when nothing waits, or one naming another call', async () => {
    const { transport } = await asked();
    await expect(transport.dispatch({ kind: 'approve', call: 't/9', scope: 'once' })).resolves.toEqual({ ok: false, refused: 'stale' });
    await transport.dispatch({ kind: 'approve', call: 't/1', scope: 'once' });
    await expect(transport.dispatch({ kind: 'approve', call: 't/1', scope: 'once' })).resolves.toEqual({ ok: false, refused: 'nothing-waiting' });
    transport.close();
  });

  it('cuts the waiting call off on a cancel: cancelled, with no decision, and the prompt gone', async () => {
    const { transport, lines, prompts } = await asked();
    await expect(transport.dispatch({ kind: 'cancel' })).resolves.toEqual({ ok: true });
    expect(prompts.at(-1)).toBeUndefined();
    await settle();
    transport.close();
    expect(calls(lines)).toEqual([expect.objectContaining({ id: 't/1', outcome: 'cancelled' })]);
    expect(calls(lines)[0]).not.toHaveProperty('approval');
    expect(lines.at(-1)).toMatchObject({ kind: 'turn.settled', reason: 'cancelled' });
  });

  it('replays the decision from the log alone: the scope, why it was held, where it ran, and the time to decide', async () => {
    const { transport, lines } = await asked();
    await new Promise((resolve) => setTimeout(resolve, 5));
    await transport.dispatch({ kind: 'approve', call: 't/1', scope: 'workspace' });
    await settle();
    transport.close();
    const [ran, refused] = tools(lines);
    expect(ran).toMatchObject({ outcome: 'ran', cwd: T1_CWD, approval: { scope: 'workspace', why: 'not_approved' } });
    expect((ran?.approval?.decided_at ?? 0) - (ran?.startedAt ?? 0)).toBeGreaterThan(0);
    expect(refused).toMatchObject({ outcome: 'refused', refusal: 'denylist', cwd: T1_CWD });
  });
});

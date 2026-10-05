import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import { APPROVAL } from '../drive/approval.ts';
import { DENYLIST, judge, judgeRequest } from './judge.ts';

/**
 * The shell gate on the page (#389, 5982832752): `diet`'s `judge_argv`
 * (#402) through its wasm build, over #402's own conformance corpus
 * (`diet/wasm/tests/shell_gate_cases.jsonl`) -- the cases the wasm and native
 * builds are held equal on -- and T1's own commands.
 */
const here = path.dirname(fileURLToPath(import.meta.url));
const read = (file: string) => readFileSync(path.join(here, '../../..', file), 'utf8');
/** Each line a request `judge_argv` takes whole -- a malformed one among them, which it must refuse. */
const CORPUS = read('diet/wasm/tests/shell_gate_cases.jsonl').trimEnd().split('\n');

describe('the shell gate, judged on the page (#389)', () => {
  it('holds the replay’s denylist to the drive’s, entry for entry', () => {
    const block = read('diet/src/drive/shell_gate.rs').match(/pub const DENYLIST: &\[&str\] = &\[([^\]]*)\];/)?.[1] ?? '';
    expect([...block.matchAll(/"([^"]*)"/g)].map((m) => m[1])).toEqual(DENYLIST);
  });

  it('answers every request of the conformance corpus: a judgement whose outcome is what its segments come to, or a refusal that says why', async () => {
    expect(CORPUS.length).toBeGreaterThan(20);
    let refusals = 0;
    for (const request of CORPUS) {
      const said = await judgeRequest(request);
      if (!said.ok) {
        refusals += 1;
        expect(said.error, request).not.toBe('');
        continue;
      }
      expect(said.judgement.segments.length, request).toBeGreaterThan(0);
      const verdicts = said.judgement.segments.map((s) => s.verdict);
      expect(said.judgement.outcome, request).toBe(verdicts.includes('refused') ? 'refused' : verdicts.includes('prompt') ? 'prompt' : 'run');
    }
    // The corpus carries requests the gate must refuse; one is not JSON at all.
    expect(refusals).toBeGreaterThan(0);
    await expect(judgeRequest('not json')).resolves.toMatchObject({ ok: false });
  });

  it('reads a wrapper through: `sh -c` with a denylisted segment after an allowed one is refused by its entry', async () => {
    await expect(judge(['sh', '-c', 'ls; sudo id'])).resolves.toEqual({
      ok: true,
      judgement: { outcome: 'refused', segments: [{ shape: 'ls', verdict: 'prompt', why: 'not_approved' }, { entry: 'sudo', verdict: 'refused' }] },
    });
  });

  it('says why it will not judge, rather than guessing', async () => {
    await expect(judge([])).resolves.toEqual({ ok: false, error: '`argv` is empty' });
  });

  it('gives T1’s commands the segments the live prompt shows for them', async () => {
    const beat = APPROVAL[1]!;
    const held = beat.events.find((e) => e.kind === 'tool.begin' && e.prompt);
    if (held?.kind !== 'tool.begin' || !held.prompt) throw new Error('the approval session holds no call');
    const said = await judge(['sh', '-c', String(held.args['command'])]);
    expect(said.ok && said.judgement.segments).toEqual(held.prompt.segments);
    await expect(judge(['sh', '-c', 'git status && git push origin main'])).resolves.toMatchObject({ ok: true, judgement: { outcome: 'refused' } });
  });
});

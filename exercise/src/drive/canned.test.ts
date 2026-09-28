import { describe, expect, it } from 'vitest';

import { CannedTransport } from './canned.ts';
import type { LogLine } from './log.ts';
import { SPECIMEN } from './specimen.ts';

describe('the canned transport, as the drive', () => {
  it('logs the idle gap a command ends just before the command, as diet will (#117)', async () => {
    const transport = new CannedTransport(SPECIMEN, { speed: 1_000_000 });
    const lines: LogLine[] = [];
    transport.subscribe((line) => lines.push(line));
    const gap = { opened_by: 0, notice: 0, read: 1000, compose: 500, away: 0, blocked: 0, ended_by: 'ask' as const };
    await transport.dispatch({ kind: 'ask', text: 'hi' }, { idle_gap: gap });
    transport.close();
    const kinds = lines.map((l) => l.kind);
    const at = kinds.indexOf('idle.gap');
    expect(at).toBeGreaterThan(-1);
    expect(kinds[at + 1]).toBe('ask');
    expect(lines[at]).toMatchObject({ ...gap, seq: at });
    expect(lines.every((l, i) => l.seq === i)).toBe(true);
  });
});

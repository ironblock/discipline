// Review probes (#733, the adversarial review of 2026-10-11), kept as the fix's proof: contract probes between the fold and what diet's session holds.
import { describe, expect, it } from 'vitest';

import { fold } from '../session/fold.ts';
import type { LogLine } from './log.ts';
import { TANGENT_OPEN } from './served/tangent.ts';

describe('a resolved entry, as diet holds it', () => {
  // diet: `Patch::Resolve` sets EntryState::Resolved, which `is_live()` is false for (diet/src/object.rs); the tangent's
  // scope is its LIVE entries born under it (diet/src/object/tangent.rs `scope`), and a close naming any other id is
  // refused `not-the-scope`.
  const resolvedInTangent: LogLine[] = [
    ...TANGENT_OPEN,
    { seq: 21, t: 105, kind: 'patch', lane: 'self-capture', op: 'resolve', entry: { id: 'e2', text: 'decision: a voxel arena' }, tangent: 't/1' },
  ] as LogLine[];

  it('is not live in working memory', () => {
    expect(fold(resolvedInTangent).memory.find((m) => m.id === 'e2')?.state).not.toBe('live');
  });

  it("is not in the open tangent's scope, which the close map must match exactly", () => {
    expect(fold(resolvedInTangent).tangent?.entries).toEqual(['e1']);
  });
});

describe('a restated entry, as diet logs it', () => {
  // diet: an `add` whose content an entry already holds is aliased to that entry (`WorkingObject::add`, Deduped) and
  // makes none; its `patch` line still names the patch's own id (`patched` in diet/src/drive/session.rs).
  const restated: LogLine[] = [
    ...TANGENT_OPEN,
    { seq: 21, t: 105, kind: 'patch', fork: 14, op: 'add', entry: { id: 'e3', text: 'decision: no login' }, tangent: 't/1' },
  ] as LogLine[];

  it('is not a second live entry, nor in the close map', () => {
    const session = fold(restated);
    expect(session.memory.filter((m) => m.state === 'live').map((m) => m.id)).toEqual(['e1', 'e2']);
    expect(session.tangent?.entries).toEqual(['e1', 'e2']);
  });
});

describe('a merged entry, as diet logs it (#872)', () => {
  // diet: under `note_dedupe = "on"`, an `add` similar to a live note of its kind is merged into it -- its id an alias of
  // that note, which gains its provenance -- and its `patch` line names the note (`merged`) and their `cosine`.
  const merged: LogLine[] = [
    ...TANGENT_OPEN,
    { seq: 21, t: 105, kind: 'patch', fork: 14, op: 'add', entry: { id: 'e3', text: 'decision: there is no login' }, tangent: 't/1', merged: 'e1', cosine: '0.7071' },
  ] as LogLine[];

  it('is a merge into the note it names, not a second live entry: that note keeps its text and shows the merge', () => {
    const session = fold(merged);
    expect(session.memory.filter((m) => m.state === 'live').map((m) => m.id)).toEqual(['e1', 'e2']);
    const into = session.memory.find((m) => m.id === 'e1');
    expect(into?.text).toBe('decision: no login');
    expect(into?.merges).toEqual([{ id: 'e3', text: 'decision: there is no login', cosine: '0.7071', by: '21' }]);
    expect(session.tangent?.entries).toEqual(['e1', 'e2']);
  });

  it('names the note it merged into thereafter', () => {
    const resolved = [...merged, { seq: 22, t: 110, kind: 'patch', fork: 14, op: 'resolve', entry: { id: 'e3', text: 'decision: there is no login' } }] as LogLine[];
    expect(fold(resolved).memory.find((m) => m.id === 'e1')?.state).toBe('resolved');
  });
});

import type { LogLine } from '../log.ts';

/**
 * Self-capture's reminder and one recorded call, WRITTEN HERE -- not recorded: no diet fixture carries `reminded` or
 * `capture` yet (#619). Turn 2's ask is followed by the reminder note; the model calls `update_record`, whose
 * `tool_call` line is beside its `capture` line (`recorded`, one entry), as #619 logs every self-elected call; then it
 * answers. One source for the fold test and the story; a fixture from diet, or a driven session, replaces it.
 */
export const SELF_CAPTURE: readonly LogLine[] = [
  { seq: 0, t: 0, kind: 'session.start', version: 7, opened: 1790000000000, model: 'a-model', head: [{ role: 'system', content: 'you are the trunk' }] },
  { seq: 1, t: 5, kind: 'ask', turn: 1, text: 'the schema has two tables' },
  { seq: 2, t: 10, kind: 'settlement', from: 'awaiting', to: 'turn' },
  { seq: 3, t: 15, kind: 'request', lane: 'trunk', turn: 1 },
  { seq: 4, t: 20, kind: 'response', to_request: 3, text: 'noted' },
  { seq: 5, t: 25, kind: 'turn.settled', turn: 1, reason: 'final' },
  { seq: 6, t: 30, kind: 'settlement', from: 'turn', to: 'awaiting' },
  { seq: 7, t: 35, kind: 'ask', turn: 2, text: 'go on' },
  { seq: 8, t: 36, kind: 'reminded', turn: 2, text: 'If this turn settled anything worth keeping, record it with update_record.' },
  { seq: 9, t: 40, kind: 'settlement', from: 'awaiting', to: 'turn' },
  { seq: 10, t: 45, kind: 'request', lane: 'trunk', turn: 2 },
  { seq: 11, t: 50, kind: 'delta', request: 10, tool_call: { index: 0, id: 'c1', name: 'update_record', arguments: '{"field":"fact","value":"the schema has two tables"}' } },
  { seq: 12, t: 55, kind: 'response', to_request: 10, text: '', finish_reason: 'tool_calls' },
  { seq: 13, t: 60, kind: 'tool_call', request: 10, turn: 2, id: 'c1', name: 'update_record', arguments: '{"field":"fact","value":"the schema has two tables"}', outcome: 'ran', stdout: 'recorded: r10/c1/fact' },
  { seq: 14, t: 60, kind: 'capture', request: 10, call: 'c1', tool: 'update_record', outcome: 'recorded', entries: ['r10/c1/fact'] },
  { seq: 15, t: 65, kind: 'request', lane: 'trunk', turn: 2 },
  { seq: 16, t: 70, kind: 'response', to_request: 15, text: 'recorded it' },
  { seq: 17, t: 75, kind: 'turn.settled', turn: 2, reason: 'final' },
  { seq: 18, t: 80, kind: 'settlement', from: 'turn', to: 'awaiting' },
] as LogLine[];

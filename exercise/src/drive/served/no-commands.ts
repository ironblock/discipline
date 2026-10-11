import type { LogLine } from '../log.ts';

/**
 * A session that runs no commands and asks no interviews, WRITTEN HERE -- not recorded: the floor regimen's start as
 * serve declares it (BEGIN's dry run, 2026-10-10), its lever table saying `approval: undeclared` (no command to
 * approve) and `fork_warrant: none`. Nothing has been asked yet.
 */
export const NO_COMMANDS: readonly LogLine[] = [
  {
    seq: 0,
    t: 0,
    kind: 'session.start',
    version: 8,
    opened: 1790000000000,
    model: 'a-model',
    head: [{ role: 'system', content: 'you are the trunk' }],
    levers: { approval: 'undeclared', fork_warrant: 'none', self_capture: 'off' },
  },
] as LogLine[];

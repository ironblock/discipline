import { APPROVED } from './approval.ts';
import { scriptAt } from './canned.ts';
import { place } from './place.ts';
import { projection } from './projection.ts';
import { SESSIONS } from './sessions.ts';
import { SCREENSHOT } from './screenshot.ts';
import { SPECIMEN } from './specimen.ts';

/**
 * Every session the surface places, as `diet check-log` is asked to read it
 * (`projection.ts`): the specimen whole, every session `?session=` replays,
 * and the two v4 sessions -- the approval prompt (#389), as its log has it
 * once approved, and the screenshot (#372) -- whole. `scripts/export-projections.mjs` writes them out for the
 * repository's `exercise` check (ruled on #300, 5974717646).
 */
export function projections(): { readonly name: string; readonly lines: readonly Record<string, unknown>[] }[] {
  return [
    { name: 'specimen', lines: projection(place(scriptAt(SPECIMEN, { beat: SPECIMEN.length })).log) },
    { name: 'approval', lines: projection(place(scriptAt(APPROVED, { beat: APPROVED.length })).log) },
    { name: 'screenshot', lines: projection(place(scriptAt(SCREENSHOT, { beat: SCREENSHOT.length })).log) },
    ...Object.entries(SESSIONS).map(([name, session]) => ({ name, lines: projection(place(session.events).log, Object.keys(session.carried)) })),
  ];
}

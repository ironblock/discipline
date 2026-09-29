import { KITCHEN_SINK } from './kitchen-sink.ts';
import { RECORDINGS } from './recorded.ts';

/**
 * Every session the app can replay by name (`?session=`): the recordings,
 * which happened, and the kitchen sink, which is authored -- the cadence a
 * working drive should have, to hold the recordings up against.
 */
export const SESSIONS = { ...RECORDINGS, 'kitchen-sink': KITCHEN_SINK } as const;

export type SessionName = keyof typeof SESSIONS;

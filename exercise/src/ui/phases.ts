/**
 * The phases the canned demo moves between: the predecessor's list. Under
 * `?drive` the composer offers the moves of the graph the log declares
 * instead (`Session.phaseMoves`, #563). Its own
 * module so that a page without a drive (the replay page) need not import
 * the app, which carries the HTTP transport.
 */
export const PHASES = ['orient', 'spec', 'plan', 'build', 'review'] as const;

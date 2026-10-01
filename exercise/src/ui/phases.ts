/**
 * The phases a person may move between. The predecessor's list, until the
 * regimen's phase graph reaches the surface with the seam (#117 R6). Its own
 * module so that a page without a drive (the replay page) need not import
 * the app, which carries the HTTP transport.
 */
export const PHASES = ['orient', 'spec', 'plan', 'build', 'review'] as const;

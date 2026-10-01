/**
 * A published recording as the page loads it: one ES module, `export default`
 * and then the recording's JSON exactly as committed. Nothing else is in the
 * file, so the scan of what is published reads the committed bytes plus this
 * one prefix (`payload.test.ts` holds that).
 */
export const PREFIX = 'export default ';

export const wrap = (json: string) => PREFIX + json;

/** The recording a payload carries, or undefined if it is not one this module wrote. */
export const unwrap = (js: string) => (js.startsWith(PREFIX) ? js.slice(PREFIX.length) : undefined);

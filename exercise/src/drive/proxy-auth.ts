/**
 * The credential the dev proxy presents to `serve` (`vite.config.ts`). A drive that runs the model's commands asks
 * for one (`diet-drive serve --auth-file`), and a page's `EventSource` and `fetch` cannot answer a Basic challenge:
 * a browser asks only for a page it navigates to. So the proxy presents it, read from the same file
 * (`DIET_DRIVE_AUTH_FILE`), and the page never holds it.
 */

/** The `Authorization` header for an auth file's TEXT, `user:password` (`serve`'s format); none for a blank file. */
export function driveAuthHeaders(text: string): { readonly Authorization: string } | undefined {
  const credential = text.trim();
  if (credential === '') return undefined;
  if (!credential.includes(':')) throw new Error('DIET_DRIVE_AUTH_FILE: expected user:password, as serve --auth-file reads it');
  return { Authorization: `Basic ${Buffer.from(credential, 'utf8').toString('base64')}` };
}

/**
 * Where the page reaches the drive (#716): one bundle, served by `serve`, by Vite's dev server, or inside a Tauri
 * webview, so nothing in it may assume it sits at the origin root of the server that answers its routes.
 *
 * - `?drive=<url>`, an http(s) URL: the drive named by hand. It overrides the meta.
 * - `<meta name="diet-drive" content="<url>">`: the drive whoever served the page named -- `serve`, or Tauri's
 *   loopback. A relative one resolves against the page's base. Its presence alone makes the page drive.
 * - Neither: same-origin, the routes relative to the page (`events`, `commands`, ...), as a page served beside them
 *   or through Vite's proxy reaches them.
 *
 * A cross-origin drive still needs `serve`'s `--allow-origin`: that rule is `serve`'s, not the page's.
 */
export interface Page {
  readonly baseURI: string;
  querySelector(selector: string): { readonly content: string } | null;
}

export function driveBase(page: Page, search: string): { readonly drive: boolean; readonly base: string } {
  const params = new URLSearchParams(search);
  // By hand, only an absolute http(s) URL names a drive: `?drive` and `?drive=1` stay the flag they always were.
  const named = params.get('drive');
  const byHand = named !== null && /^https?:\/\//i.test(named) ? asBase(named, page.baseURI) : undefined;
  if (byHand) return { drive: true, base: byHand };
  const meta = page.querySelector('meta[name="diet-drive"]')?.content.trim();
  const injected = meta ? asBase(meta, page.baseURI) : undefined;
  if (injected) return { drive: true, base: injected };
  return { drive: params.has('drive'), base: '' };
}

/** `text` resolved against `base` as an http(s) URL ending in `/` (routes join under it); anything else is none. */
function asBase(text: string, base: string): string | undefined {
  let url: URL;
  try {
    url = new URL(text, base);
  } catch {
    return undefined;
  }
  if (url.protocol !== 'http:' && url.protocol !== 'https:') return undefined;
  return url.href.endsWith('/') ? url.href : `${url.href}/`;
}

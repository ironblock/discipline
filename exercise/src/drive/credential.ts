/**
 * A page that holds the drive's credential itself (#719). A drive that runs the model's commands asks for one
 * (`diet-drive serve --auth-file`). In the browser, Vite's proxy presents it and the page never holds it
 * (`proxy-auth.ts`). The native shell has no proxy: it hands the page the `Authorization` value at run time, over its
 * own IPC, so the credential is never in a URL, the bundle or a file the page writes.
 *
 * With it, every fetch carries the header, and `/events` is read by a streamed fetch (`StreamedSource`), since an
 * `EventSource` cannot send one. `serve` admits the header across origins (its preflight allows `authorization`).
 */

import type { EventSourceLike, Web } from './http.ts';

type Fetch = (url: string, init?: RequestInit) => Promise<Response>;

const CONNECTING = 0;
const OPEN = 1;
const CLOSED = 2;

/** The browser's own pieces, with every request carrying `authorization`. */
export function credentialedWeb(authorization: string, fetch: Fetch = globalThis.fetch.bind(globalThis)): Web {
  const withCredential: Fetch = async (url, init) => {
    const headers = new Headers(init?.headers);
    headers.set('Authorization', authorization);
    return fetch(url, { ...init, headers });
  };
  return {
    fetch: withCredential,
    EventSource: class extends StreamedSource {
      constructor(url: string) {
        super(url, {}, withCredential);
      }
    },
  };
}

/**
 * Server-sent events read by a fetch, as much of `EventSource` as the transport uses. Unlike the browser's, it never
 * reconnects by itself: a stream that ends or is refused closes, with an error, and the transport's probe decides what
 * comes next (finding 17 in `http.ts`), resuming by `?from=`.
 */
export class StreamedSource implements EventSourceLike {
  readyState = CONNECTING;
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent<string>) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  readonly #named = new Map<string, ((event: MessageEvent<string>) => void)[]>();
  readonly #abort = new AbortController();
  /** The last `id:` seen; it carries over to events that name none, as an `EventSource`'s does. */
  #lastId = '';

  constructor(url: string, headers: HeadersInit, fetch: Fetch) {
    void this.#read(url, headers, fetch);
  }

  addEventListener(type: string, listener: (event: MessageEvent<string>) => void): void {
    this.#named.set(type, [...(this.#named.get(type) ?? []), listener]);
  }

  close(): void {
    this.readyState = CLOSED;
    this.#abort.abort();
  }

  async #read(url: string, given: HeadersInit, fetch: Fetch): Promise<void> {
    const headers = new Headers(given);
    headers.set('Accept', 'text/event-stream');
    try {
      const reply = await fetch(url, { headers, signal: this.#abort.signal });
      if (this.readyState === CLOSED) return;
      if (reply.status !== 200 || !reply.body) return this.#fail();
      this.readyState = OPEN;
      this.onopen?.(new Event('open'));
      const reader = reply.body.pipeThrough(new TextDecoderStream()).getReader();
      let pending = '';
      let event = { type: '', data: [] as string[] };
      for (;;) {
        const { done, value } = await reader.read();
        if (this.readyState === CLOSED) return;
        if (done) return this.#fail();
        pending += value;
        const lines = pending.split(/\r\n|\r|\n/);
        pending = lines.pop() ?? '';
        for (const line of lines) {
          if (line === '') {
            if (event.data.length > 0) this.#dispatch(event.type || 'message', event.data.join('\n'));
            event = { type: '', data: [] };
            continue;
          }
          if (line.startsWith(':')) continue;
          const colon = line.indexOf(':');
          const field = colon === -1 ? line : line.slice(0, colon);
          const raw = colon === -1 ? '' : line.slice(colon + 1);
          const said = raw.startsWith(' ') ? raw.slice(1) : raw;
          if (field === 'data') event.data.push(said);
          else if (field === 'event') event.type = said;
          else if (field === 'id' && !said.includes('\0')) this.#lastId = said;
        }
      }
    } catch {
      if (this.readyState !== CLOSED) this.#fail();
    }
  }

  #dispatch(type: string, data: string): void {
    const event = new MessageEvent(type, { data, lastEventId: this.#lastId });
    if (type === 'message') this.onmessage?.(event);
    for (const listener of this.#named.get(type) ?? []) listener(event);
  }

  #fail(): void {
    this.readyState = CLOSED;
    this.onerror?.(new Event('error'));
  }
}

/** What the native shell exposes to its page: Tauri's IPC. Absent in a browser. */
interface Shell {
  readonly __TAURI_INTERNALS__?: { readonly invoke: (command: string) => Promise<unknown> };
}

/** The `Authorization` value the native shell holds for its drive, or none: in a browser, or a drive that asks none. */
export async function shellAuthorization(scope: Shell = globalThis as Shell): Promise<string | undefined> {
  const invoke = scope.__TAURI_INTERNALS__?.invoke;
  if (!invoke) return undefined;
  const answer = await invoke('drive_authorization');
  return typeof answer === 'string' && answer !== '' ? answer : undefined;
}

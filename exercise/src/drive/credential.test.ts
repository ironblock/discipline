import { describe, expect, it } from 'vitest';

import { credentialedWeb, shellAuthorization, StreamedSource } from './credential.ts';
import { HttpTransport } from './http.ts';
import type { LogLine } from './log.ts';

/**
 * A page that holds the drive's credential itself (#719: the native shell hands it over at run time): every fetch
 * carries it, and `/events` is read by a streamed fetch, since an `EventSource` cannot send a header.
 */
const AUTH = 'Basic dXNlcjpwYXNz';
const OPENED = 1_790_000_000_000;
const settle = async () => new Promise((resolve) => setTimeout(resolve, 0));

/** A body the test writes into by hand, as serve's stream arrives: in chunks that need not end on a line. */
function stream() {
  let push: ReadableStreamDefaultController<Uint8Array> | undefined;
  const body = new ReadableStream<Uint8Array>({ start: (controller) => void (push = controller) });
  const encoder = new TextEncoder();
  return { body, write: (text: string) => push?.enqueue(encoder.encode(text)), end: () => push?.close() };
}

function fetching(answer: (url: string, init?: RequestInit) => Response) {
  const fetched: { url: string; headers: Headers }[] = [];
  const fetch = async (url: string, init?: RequestInit) => {
    fetched.push({ url, headers: new Headers(init?.headers) });
    return answer(url, init);
  };
  return { fetch, fetched };
}

describe('credentialedWeb: every request carries the credential', () => {
  it('adds Authorization to a fetch and keeps the headers it was given', async () => {
    const { fetch, fetched } = fetching(() => new Response('{}'));
    const web = credentialedWeb(AUTH, fetch);
    await web.fetch('commands', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' });
    expect(fetched[0]?.headers.get('authorization')).toBe(AUTH);
    expect(fetched[0]?.headers.get('content-type')).toBe('application/json');
  });

  it('reads /events with the credential, as a stream', async () => {
    const { body } = stream();
    const { fetch, fetched } = fetching(() => new Response(body, { headers: { 'Content-Type': 'text/event-stream' } }));
    const source = new (credentialedWeb(AUTH, fetch).EventSource)('events?from=0');
    await settle();
    expect(fetched[0]?.url).toBe('events?from=0');
    expect(fetched[0]?.headers.get('authorization')).toBe(AUTH);
    expect(fetched[0]?.headers.get('accept')).toBe('text/event-stream');
    source.close();
  });
});

describe('StreamedSource: server-sent events over a fetch', () => {
  it('opens, then dispatches each event: its data, its id, and named ones to their listeners', async () => {
    const { body, write } = stream();
    const { fetch } = fetching(() => new Response(body));
    const source = new StreamedSource('events?from=0', {}, fetch);
    const seen: string[] = [];
    source.onopen = () => seen.push(`open ${source.readyState}`);
    source.onmessage = (event) => seen.push(`message ${event.lastEventId} ${event.data}`);
    source.addEventListener('waiting', (event) => seen.push(`waiting ${event.data}`));
    expect(source.readyState).toBe(0);
    await settle();
    // A line split across chunks, a CRLF, a two-line data, and a named event with no id: the last id carries over.
    write('id: 7-0\ndata: {"seq":');
    write('0}\n\nid: 7-1\r\ndata: a\ndata: b\n\nevent: waiting\ndata: {"id":"c"}\n\n: a comment\n\n');
    await settle();
    expect(seen).toEqual(['open 1', 'message 7-0 {"seq":0}', 'message 7-1 a\nb', 'waiting {"id":"c"}']);
    source.close();
  });

  it('is closed, with an error, when the drive answers anything but 200', async () => {
    const { fetch } = fetching(() => new Response(null, { status: 401 }));
    const source = new StreamedSource('events?from=0', {}, fetch);
    const seen: string[] = [];
    source.onopen = () => seen.push('open');
    source.onerror = () => seen.push(`error ${source.readyState}`);
    await settle();
    expect(seen).toEqual(['error 2']);
  });

  it('is closed, with an error, when the stream ends or cannot be reached', async () => {
    const { body, end } = stream();
    const ended = new StreamedSource('events', {}, fetching(() => new Response(body)).fetch);
    const unreachable = new StreamedSource('events', {}, async () => Promise.reject(new TypeError('Failed to fetch')));
    const seen: string[] = [];
    ended.onerror = () => seen.push(`ended ${ended.readyState}`);
    unreachable.onerror = () => seen.push(`unreachable ${unreachable.readyState}`);
    await settle();
    end();
    await settle();
    expect(seen.sort()).toEqual(['ended 2', 'unreachable 2']);
  });

  it('once closed by the page, aborts its fetch and says nothing more', async () => {
    const { body, write } = stream();
    let signal: AbortSignal | undefined;
    const source = new StreamedSource('events', {}, async (_url, init) => {
      signal = init?.signal ?? undefined;
      return new Response(body);
    });
    const seen: string[] = [];
    source.onmessage = (event) => seen.push(event.data);
    source.onerror = () => seen.push('error');
    await settle();
    source.close();
    write('data: late\n\n');
    await settle();
    expect(signal?.aborted).toBe(true);
    expect(source.readyState).toBe(2);
    expect(seen).toEqual([]);
  });
});

describe('the transport over a credentialed web', () => {
  it('reads the log a credentialed stream serves', async () => {
    const { body, write } = stream();
    const { fetch, fetched } = fetching(() => new Response(body));
    const transport = new HttpTransport('http://127.0.0.1:7801/', credentialedWeb(AUTH, fetch));
    const lines: LogLine[] = [];
    const links: string[] = [];
    transport.watchLink((link) => links.push(link));
    transport.subscribe((line) => lines.push(line));
    await settle();
    const start = { seq: 0, t: 0, kind: 'session.start', version: 0, opened: OPENED, model: 'm', head: [] };
    write(`id: ${OPENED}-0\ndata: ${JSON.stringify(start)}\n\n`);
    await settle();
    expect(fetched[0]?.url).toBe('http://127.0.0.1:7801/events?from=0');
    expect(fetched[0]?.headers.get('authorization')).toBe(AUTH);
    expect(links).toContain('live');
    expect(lines.map((line) => line.kind)).toEqual(['session.start']);
    transport.close();
  });
});

describe('shellAuthorization: the credential the native shell holds', () => {
  it('is none in a browser, where no shell answers', async () => {
    expect(await shellAuthorization({})).toBeUndefined();
  });

  it('is what the shell answers, and none when it holds none', async () => {
    const asked: string[] = [];
    const shell = (answer: unknown) => ({
      __TAURI_INTERNALS__: {
        invoke: async (command: string) => {
          asked.push(command);
          return answer;
        },
      },
    });
    expect(await shellAuthorization(shell(AUTH))).toBe(AUTH);
    expect(await shellAuthorization(shell(null))).toBeUndefined();
    expect(asked).toEqual(['drive_authorization', 'drive_authorization']);
  });
});

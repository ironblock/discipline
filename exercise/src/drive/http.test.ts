import { describe, expect, it } from 'vitest';

import type { EventSourceLike, Web } from './http.ts';
import { HttpTransport } from './http.ts';
import type { LogLine } from './log.ts';

/**
 * `HttpTransport` against a stand-in for `serve.rs`: an EventSource the test
 * drives by hand, and a fetch that answers what the test says. The contract
 * is serve.rs's (#128); finding 17 (#117) is what a closed stream does.
 */
const OPENED = 1_790_000_000_000;

class FakeSource implements EventSourceLike {
  readyState = 0;
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent<string>) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  readonly named = new Map<string, ((event: MessageEvent<string>) => void)[]>();
  closed = false;
  constructor(readonly url: string) {}
  addEventListener(type: string, listener: (event: MessageEvent<string>) => void): void {
    this.named.set(type, [...(this.named.get(type) ?? []), listener]);
  }
  /** A named event (`waiting`, `answered`): no `id:`, as serve sends them (#389). */
  emit(type: string, data: Record<string, unknown>): void {
    for (const listener of this.named.get(type) ?? []) listener(new MessageEvent(type, { data: JSON.stringify(data) }));
  }
  open(): void {
    this.readyState = 1;
    this.onopen?.(new Event('open'));
  }
  send(line: Record<string, unknown>, opened = OPENED): void {
    this.onmessage?.(new MessageEvent('message', { data: JSON.stringify(line), lastEventId: `${opened}-${String(line['seq'])}` }));
  }
  /** The browser still retrying (0) or given up (2). */
  fail(state: 0 | 2): void {
    this.readyState = state;
    this.onerror?.(new Event('error'));
  }
  close(): void {
    this.closed = true;
    this.readyState = 2;
  }
}

function stand(answers: { readonly events?: number; readonly commands?: (body: Record<string, unknown>) => Response } = {}) {
  const sources: FakeSource[] = [];
  const fetched: { url: string; init?: RequestInit }[] = [];
  const web: Web = {
    EventSource: class extends FakeSource {
      constructor(url: string) {
        super(url);
        sources.push(this);
      }
    },
    fetch: async (url, init) => {
      fetched.push({ url, ...(init ? { init } : {}) });
      if (url.includes('/events')) return new Response(null, { status: answers.events ?? 200 });
      const body = JSON.parse(String(init?.body ?? '{}')) as Record<string, unknown>;
      return answers.commands ? answers.commands(body) : new Response('{}', { status: 200 });
    },
  };
  const transport = new HttpTransport('', web);
  const lines: LogLine[] = [];
  const links: string[] = [];
  transport.watchLink((link, why) => links.push(why ? `${link}: ${why}` : link));
  transport.subscribe((line) => lines.push(line));
  return { transport, sources, fetched, lines, links, last: () => sources.at(-1)! };
}

const start = { seq: 0, t: 0, kind: 'session.start', version: 0, opened: OPENED, model: 'm', head: [{ role: 'system', content: 's' }] };
const ask = (seq: number, turn = 1) => ({ seq, t: seq, kind: 'ask', turn, text: 'hi' });
const settle = async () => new Promise((resolve) => setTimeout(resolve, 0));

describe('HttpTransport: the log, served', () => {
  it('reads the log from 0, in order, and is live once the stream opens', () => {
    const { sources, lines, links, last } = stand();
    expect(sources.map((s) => s.url)).toEqual(['/events?from=0']);
    expect(links).toEqual(['reconnecting']);
    last().open();
    last().send(start);
    last().send(ask(1));
    expect(lines.map((l) => l.seq)).toEqual([0, 1]);
    expect(links.at(-1)).toBe('live');
  });

  it('keeps the log gapless: an overlap is dropped, and so is a line past a gap', () => {
    const { lines, last } = stand();
    last().send(start);
    last().send(start);
    last().send(ask(3));
    last().send(ask(1));
    expect(lines.map((l) => l.seq)).toEqual([0, 1]);
  });

  it('says it is reconnecting while the browser retries a dropped stream, and makes no new one', () => {
    const { sources, links, last } = stand();
    last().open();
    last().fail(0);
    expect(links.at(-1)).toBe('reconnecting');
    expect(sources).toHaveLength(1);
  });
});

describe('HttpTransport: a closed stream (finding 17)', () => {
  it('asks, once, what the resume is answered with -- and shows a refusal to the author, why and all', async () => {
    const { sources, fetched, links, last } = stand({ events: 403 });
    last().open();
    last().send(start);
    last().fail(2);
    await settle();
    expect(fetched).toHaveLength(1);
    expect(fetched[0]?.url).toBe('/events?from=1');
    expect(fetched[0]?.init?.headers).toEqual({ 'Last-Event-ID': `${OPENED}-0` });
    expect(links.at(-1)).toMatch(/^lost: .*--allow-origin.*\(403\)$/);
    // Not retried blind.
    expect(sources).toHaveLength(1);
  });

  it('keeps one stream when subscribed again while a probe is out, and does not hear the one it let go', async () => {
    const { transport, sources, lines, last } = stand({ events: 200 });
    last().open();
    last().send(start);
    last().fail(2);
    // The probe is out: a remount subscribes, and connects.
    transport.subscribe(() => undefined);
    await settle();
    expect(sources).toHaveLength(2);
    sources[0]?.send(ask(1));
    expect(lines).toHaveLength(1);
    last().send(ask(1));
    expect(lines).toHaveLength(2);
  });

  it('rebuilds on 410 -- another process, a new session -- from its first line', async () => {
    const { sources, lines, last } = stand({ events: 410 });
    last().send(start);
    last().send(ask(1));
    last().fail(2);
    await settle();
    expect(sources.map((s) => s.url)).toEqual(['/events?from=0', '/events?from=0']);
    last().send({ ...start, opened: OPENED + 5 }, OPENED + 5);
    expect(lines.map((l) => l.seq)).toEqual([0, 1, 0]);
  });

  it('shows an unreachable drive, and each other status, as lost', async () => {
    const { links, last } = stand({ events: 503 });
    last().fail(2);
    await settle();
    expect(links.at(-1)).toMatch(/^lost: .*\(503\)$/);
  });

  it('resumes when the resume is answered 200, and calls it lost after three that will not stay open', async () => {
    const { sources, links } = stand({ events: 200 });
    for (let i = 0; i < 4; i += 1) {
      sources.at(-1)!.fail(2);
      await settle();
    }
    expect(sources).toHaveLength(4);
    expect(links.at(-1)).toBe('lost: the drive answers, but its stream keeps closing');
  });

  it('starts over when an id names another session: a restart a fresh resume could not name', () => {
    const { sources, lines, last } = stand();
    last().send(start);
    last().send(ask(1));
    last().send(ask(2), OPENED + 5);
    expect(sources.map((s) => s.url)).toEqual(['/events?from=0', '/events?from=0']);
    expect(lines.map((l) => l.seq)).toEqual([0, 1]);
  });
});

describe('HttpTransport: commands', () => {
  it('posts an ask as serve.rs takes it', async () => {
    const posted: Record<string, unknown>[] = [];
    const { transport } = stand({ commands: (body) => (posted.push(body), new Response('{"seq":1,"turn":1}', { status: 200 })) });
    await expect(transport.dispatch({ kind: 'ask', text: 'hi' })).resolves.toEqual({ ok: true });
    expect(posted).toEqual([{ kind: 'ask', text: 'hi' }]);
  });

  it('carries a refusal by its tag', async () => {
    const { transport } = stand({ commands: () => new Response('{"refused":"in-flight"}', { status: 409 }) });
    await expect(transport.dispatch({ kind: 'ask', text: 'hi' })).resolves.toEqual({ ok: false, refused: 'in-flight' });
  });

  it('stops the latest turn asked; with none, refuses without posting', async () => {
    const posted: Record<string, unknown>[] = [];
    const { transport, last } = stand({ commands: (body) => (posted.push(body), new Response('{}', { status: 200 })) });
    await expect(transport.dispatch({ kind: 'cancel' })).resolves.toEqual({ ok: false, refused: 'nothing-in-flight' });
    last().send(start);
    last().send(ask(1, 1));
    last().send(ask(2, 2));
    await transport.dispatch({ kind: 'cancel' });
    expect(posted).toEqual([{ kind: 'cancel', turn: 2 }]);
  });

  it('declares a seam without a phase: which one is the drive’s to say', async () => {
    const posted: Record<string, unknown>[] = [];
    const { transport } = stand({ commands: (body) => (posted.push(body), new Response('{}', { status: 200 })) });
    await transport.dispatch({ kind: 'seam', to: 'build' });
    expect(posted).toEqual([{ kind: 'declare-seam' }]);
  });

  it('posts an end as serve.rs takes it (#289)', async () => {
    const posted: Record<string, unknown>[] = [];
    const { transport } = stand({ commands: (body) => (posted.push(body), new Response('{}', { status: 200 })) });
    await expect(transport.dispatch({ kind: 'end' })).resolves.toEqual({ ok: true });
    expect(posted).toEqual([{ kind: 'end' }]);
  });
});

describe('HttpTransport: closed and reopened', () => {
  it('resumes from the log it holds when subscribed again after close (a remount)', () => {
    const { transport, sources, last } = stand();
    last().send(start);
    last().send(ask(1));
    transport.close();
    expect(last().closed).toBe(true);
    const again: LogLine[] = [];
    transport.subscribe((line) => again.push(line));
    expect(again.map((l) => l.seq)).toEqual([0, 1]);
    expect(sources.map((s) => s.url)).toEqual(['/events?from=0', '/events?from=2']);
  });
});

describe('HttpTransport: the idle gap a command ends (Q4, #146)', () => {
  const gap = { opened_by: 7, notice: 100, read: 2000, compose: 900, away: 0, blocked: 0, ended_by: 'ask' as const };

  it('sends it on the command, as serve.rs takes it', async () => {
    const posted: Record<string, unknown>[] = [];
    const { transport } = stand({ commands: (body) => (posted.push(body), new Response('{}', { status: 200 })) });
    await expect(transport.dispatch({ kind: 'ask', text: 'hi' }, { idle_gap: gap })).resolves.toEqual({ ok: true });
    expect(posted).toEqual([{ kind: 'ask', text: 'hi', idle_gap: gap }]);
  });

  it('sends the command again without it when diet will not log it (400): a measurement never costs the ask', async () => {
    const posted: Record<string, unknown>[] = [];
    const { transport } = stand({ commands: (body) => (posted.push(body), new Response('{}', { status: 'idle_gap' in body ? 400 : 200 })) });
    await expect(transport.dispatch({ kind: 'ask', text: 'hi' }, { idle_gap: gap })).resolves.toEqual({ ok: true });
    expect(posted).toEqual([{ kind: 'ask', text: 'hi', idle_gap: gap }, { kind: 'ask', text: 'hi' }]);
  });

  it('does not resend a refusal: a refused command drops its gap, and diet logs none', async () => {
    const posted: Record<string, unknown>[] = [];
    const { transport } = stand({ commands: (body) => (posted.push(body), new Response('{"refused":"in-flight"}', { status: 409 })) });
    await expect(transport.dispatch({ kind: 'ask', text: 'hi' }, { idle_gap: gap })).resolves.toEqual({ ok: false, refused: 'in-flight' });
    expect(posted).toHaveLength(1);
  });
});

describe('HttpTransport: a tool call’s file, by digest (#372)', () => {
  const sha = 'ab'.repeat(32);
  const serving = (status: number, body: BodyInit | null = null) => {
    const asked: string[] = [];
    const web: Web = {
      EventSource: class extends FakeSource {},
      fetch: (url) => (asked.push(url), Promise.resolve(new Response(body, { status }))),
    };
    return { transport: new HttpTransport('', web), asked };
  };

  it('asks serve for GET /files/<sha256> and answers its bytes, unchecked', async () => {
    const { transport, asked } = serving(200, new Uint8Array([1, 2, 3]));
    await expect(transport.file(sha)).resolves.toEqual({ kind: 'bytes', bytes: new Uint8Array([1, 2, 3]) });
    expect(asked).toEqual([`/files/${sha}`]);
  });

  it('answers not found on 404, and why on anything else', async () => {
    await expect(serving(404).transport.file(sha)).resolves.toEqual({ kind: 'not-found' });
    await expect(serving(503).transport.file(sha)).resolves.toEqual({ kind: 'unreachable', why: 'the drive is at its connection limit, or down (503)' });
  });

  it('never asks for what is not a digest', async () => {
    const { transport, asked } = serving(200, 'x');
    await expect(transport.file('../../etc/passwd')).resolves.toEqual({ kind: 'not-found' });
    // As a digest is written: lowercase, as the replay's source reads it too.
    await expect(transport.file(sha.toUpperCase())).resolves.toEqual({ kind: 'not-found' });
    expect(asked).toEqual([]);
  });
});

describe('HttpTransport: a call waiting on the operator (#389, ruled 5982826097)', () => {
  const waiting = { request: 3, id: 'call_1', command: 'npm install', cwd: '~/git/experiments/t1', reason: 'not_approved', segments: [{ shape: 'npm install', verdict: 'prompt', why: 'not_approved' }] };
  const watched = (transport: HttpTransport) => {
    const seen: unknown[] = [];
    transport.watchPrompt((prompt) => seen.push(prompt?.id));
    return seen;
  };

  it('shows the waiting event, and clears it on answered', () => {
    const { transport, last } = stand();
    const seen = watched(transport);
    last().emit('waiting', waiting);
    last().emit('answered', { request: 3, id: 'call_1' });
    expect(seen).toEqual([undefined, 'call_1', undefined]);
  });

  it('clears it on the call’s tool_call line, found by request and id', () => {
    const { transport, last } = stand();
    const seen = watched(transport);
    last().send(start);
    last().emit('waiting', waiting);
    last().send({ seq: 1, t: 9, kind: 'tool_call', request: 2, turn: 1, id: 'call_1', name: 'bash', arguments: '{}', outcome: 'refused', reason: 'declined', argv: ['npm', 'install'] });
    expect(seen).toEqual([undefined, 'call_1']);
    last().send({ seq: 2, t: 9, kind: 'tool_call', request: 3, turn: 1, id: 'call_1', name: 'bash', arguments: '{}', outcome: 'refused', reason: 'declined', argv: ['npm', 'install'] });
    expect(seen).toEqual([undefined, 'call_1', undefined]);
  });

  it('posts an answer to /approve as {call, scope}, and clears the prompt on its ack', async () => {
    const posted: { url: string; body: Record<string, unknown> }[] = [];
    const { transport, last, fetched } = stand({ commands: (body) => (posted.push({ url: fetched.at(-1)!.url, body }), new Response(null, { status: 204 })) });
    const seen = watched(transport);
    last().emit('waiting', waiting);
    await expect(transport.dispatch({ kind: 'approve', call: 'call_1', scope: 'session' })).resolves.toEqual({ ok: true });
    expect(posted).toEqual([{ url: '/approve', body: { call: 'call_1', scope: 'session' } }]);
    expect(seen).toEqual([undefined, 'call_1', undefined]);
  });

  it('keeps the prompt when the answer is refused, and carries the refusal by its tag', async () => {
    const { transport, last } = stand({ commands: () => new Response('{"refused":"stale"}', { status: 409 }) });
    const seen = watched(transport);
    last().emit('waiting', waiting);
    await expect(transport.dispatch({ kind: 'approve', call: 'call_1', scope: 'decline' })).resolves.toEqual({ ok: false, refused: 'stale' });
    expect(seen).toEqual([undefined, 'call_1']);
  });

  it('does not show a waiting event that does not read as a prompt', () => {
    const { transport, last } = stand();
    const seen = watched(transport);
    last().emit('waiting', { id: 'call_1' });
    expect(seen).toEqual([undefined]);
  });
});

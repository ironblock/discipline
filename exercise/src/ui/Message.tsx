import { useState } from 'react';

import type { AssistantNode, Folded, SettledNode, SystemNode, UserNode } from '../session/fold.ts';
import { Block } from './Block.tsx';
import { counter, ms, rate, tokens } from './format.ts';
import { Copy } from './Copy.tsx';
import { Prose } from './Prose.tsx';
import { alarmOf, failOf, settleOf, stopOf } from './sets.ts';
import { elapsed, useNow } from './surface.tsx';
import './message.css';

/**
 * The first lines of a text, cut where Markdown allows: never inside a code
 * block the preview opened, which would render as an empty box.
 */
export function preview(text: string, lines: number): { readonly shown: string; readonly hidden: number } {
  const all = text.split('\n');
  let cut = Math.min(lines, all.length);
  const fences = all.slice(0, cut).filter((line) => /^\s*(```|~~~)/.test(line)).length;
  if (fences % 2 === 1) {
    const opened = all.slice(0, cut).findLastIndex((line) => /^\s*(```|~~~)/.test(line));
    if (opened > 0) cut = opened;
  }
  return { shown: all.slice(0, cut).join('\n'), hidden: all.length - cut };
}

/** The trunk's system prompt: the phase's priming, or working memory rendered. */
export function SystemMessage({ node }: { readonly node: Folded<SystemNode> }) {
  const [open, setOpen] = useState(false);
  const clipped = preview(node.text, 3);
  const shown = open ? node.text : clipped.shown;
  return (
    <Block
      tone="system"
      label={node.render === undefined ? 'system' : `system · render v${node.render}`}
      stats={[node.tokens !== undefined && { value: tokens(node.tokens), unit: 'tok', title: 'tokens in the prefix' }]}
      provenance={node}
      id={node.id}
    >
      <Prose text={shown} kind="system" />
      {clipped.hidden > 0 ? (
        <button type="button" className="ex-more" onClick={() => setOpen(!open)}>
          {open ? 'less' : `${clipped.hidden} more lines`}
        </button>
      ) : null}
    </Block>
  );
}

/** A person's ask. Its footer says what it cost: the prefill it caused, new and reused. */
export function UserMessage({ node }: { readonly node: Folded<UserNode> }) {
  return (
    <Block
      tone="user"
      label="user"
      stats={
        node.prefill
          ? [
              { value: tokens(node.prefill.fresh), unit: 'new', title: 'prompt tokens this ask caused to be evaluated' },
              { value: tokens(node.prefill.cached), unit: 'cached', title: 'prompt tokens reused from the slot' },
            ]
          : []
      }
      provenance={node}
      id={node.id}
      actions={<Copy text={node.text} />}
    >
      <Prose text={node.text} kind="ask" />
    </Block>
  );
}

/** The model on the trunk: its reasoning in italic, then its answer, streamed. */
export function AssistantMessage({ node }: { readonly node: Folded<AssistantNode> }) {
  const [thinking, setThinking] = useState(false);
  const live = node.progress === 'prefill' || node.progress === 'streaming';
  const t = node.timings;
  const long = node.reasoning.length > 280;
  const showReasoning = node.reasoning !== '' && (thinking || !long || (live && node.text === ''));
  const now = useNow();
  const since = elapsed(now, node.startedAt);
  // Prefill is silent by nature, and a long one is the cost worth flagging: it
  // escalates on its total. A generation escalates only on silence -- time
  // since its last token -- never while tokens are arriving.
  const worry = node.progress === 'prefill' ? since.level : elapsed(now, node.lastActivityAt).level;
  const streamingInto = node.progress === 'streaming' ? (node.text === '' ? 'reasoning' : 'answer') : undefined;
  // A turn that said nothing and only called a tool: a step, not a message.
  const silent = node.progress === 'done' && node.text === '' && node.reasoning === '';
  const stopped = stopOf(node.stop ?? 'stop');
  return (
    <Block
      tone="assistant"
      label={silent ? 'assistant · a call, no text' : 'assistant'}
      thin={silent}
      live={live}
      alarm={node.failure ? 'bad' : alarmOf(stopped.level)}
      stats={
        node.failure
          ? [
              node.wallMs !== undefined && { value: ms(node.wallMs), title: 'request to failure, wall clock' },
              {
                value: (
                  <span className="ex-failure" data-level={failOf(node.failure.reason).level}>
                    failed · {failOf(node.failure.reason).label}
                  </span>
                ),
                title: node.failure.message,
              },
            ]
          : t
          ? [
              stopped.level !== 'ok' && {
                value: (
                  <span className="ex-stop" data-level={stopped.level}>
                    {stopped.label}
                  </span>
                ),
                title: 'why generation stopped',
              },
              node.wallMs !== undefined && { value: ms(node.wallMs), title: 'request to response, wall clock' },
              { value: tokens(t.predicted_n), unit: 'tok', title: 'tokens generated, reasoning included' },
              { value: rate(t.predicted_n, t.predicted_ms), unit: 'tg t/s', title: 'generation speed' },
              { value: tokens(t.prompt_n), unit: 'new', title: 'prompt tokens evaluated' },
              { value: rate(t.prompt_n, t.prompt_ms), unit: 'pp t/s', title: 'prefill speed' },
            ]
          : [
              {
                value: (
                  <span className="ex-elapsed" data-level={worry}>
                    {node.progress === 'prefill' ? 'prefill' : 'generating'} · {counter(since.ms)}
                  </span>
                ),
                title: 'how long since the request',
              },
            ]
      }
      provenance={node}
      id={node.id}
      {...(node.text !== '' && !live ? { actions: <Copy text={node.text} /> } : {})}
    >
      {silent ? undefined : <AssistantBody node={node} live={live} long={long} showReasoning={showReasoning} thinking={thinking} setThinking={setThinking} streamingInto={streamingInto} />}
    </Block>
  );
}

function AssistantBody({
  node,
  live,
  long,
  showReasoning,
  thinking,
  setThinking,
  streamingInto,
}: {
  readonly node: Folded<AssistantNode>;
  readonly live: boolean;
  readonly long: boolean;
  readonly showReasoning: boolean;
  readonly thinking: boolean;
  readonly setThinking: (next: boolean) => void;
  readonly streamingInto: 'reasoning' | 'answer' | undefined;
}) {
  return (
    <>
      {node.reasoning !== '' ? (
        <div className="ex-reasoning">
          {showReasoning ? (
            <Prose text={node.reasoning} kind="reasoning" caret={streamingInto === 'reasoning'} />
          ) : (
            <div className="ex-reasoning__clip">
              <Prose text={node.reasoning.slice(0, 200) + '…'} kind="reasoning" />
            </div>
          )}
          {long && !(live && node.text === '') ? (
            <button type="button" className="ex-more" onClick={() => setThinking(!thinking)}>
              {thinking ? 'less' : 'all reasoning'}
            </button>
          ) : null}
        </div>
      ) : null}
      {node.progress === 'prefill' ? (
        <div className="ex-waiting" role="status">
          <span className="ex-waiting__line" />
          <span className="ex-waiting__line" />
          <span className="ex-waiting__label">reading the prompt</span>
        </div>
      ) : null}
      {node.text !== '' ? <Prose text={node.text} kind="answer" caret={streamingInto === 'answer'} /> : null}
      {node.progress === 'cancelled' ? <p className="ex-cancelled">cancelled</p> : null}
      {node.failure ? (
        <p className="ex-failed" role="alert">
          <span className="ex-failed__reason">{failOf(node.failure.reason).label}</span>
          <span className="ex-failed__message">{node.failure.message}</span>
        </p>
      ) : null}
    </>
  );
}

/**
 * The end of a turn that did not end on its own -- the step limit, a timeout,
 * a reason from a newer drive -- drawn across the trunk where the turn stopped.
 */
export function TurnEnd({ node }: { readonly node: Folded<SettledNode> }) {
  const settled = settleOf(node.reason);
  return (
    <div
      className="ex-turnend"
      role="note"
      data-level={settled.level}
      data-alarm={alarmOf(settled.level)}
      data-known={settled.known ? '' : undefined}
      data-id={node.id}
      data-from={node.from.join(' ')}
      data-needs={node.needs.join(' ')}
    >
      <span className="ex-turnend__rule" aria-hidden="true" />
      <span>
        turn {node.turn} ended · <strong>{settled.label}</strong>
      </span>
      <span className="ex-turnend__rule" aria-hidden="true" />
    </div>
  );
}

import { useState } from 'react';

import type { AssistantNode, Folded, SettledNode, SystemNode, ToolNode, UserNode } from '../session/fold.ts';
import { Block } from './Block.tsx';
import { tokens, took } from './format.ts';
import { Copy } from './Copy.tsx';
import { edgeOf, readingOf, warmOf, writingOf, writtenApart } from './flow.ts';
import { Flowing } from './Flowing.tsx';
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
      {...(node.tokens !== undefined ? { input: <span title="tokens in the prefix">{tokens(node.tokens)} tok</span> } : {})}
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

/** A person's ask. Its header says what it will cost to read: the new tokens it put in front of the model. */
export function UserMessage({ node }: { readonly node: Folded<UserNode> }) {
  return (
    <Block
      tone="user"
      label="user"
      {...(node.prefill
        ? {
            input: (
              <span title={`new tokens this ask put in front of the model; ${tokens(node.prefill.cached)} before it were warm, reused from the slot`}>
                +{tokens(node.prefill.fresh)} tok
              </span>
            ),
          }
        : {})}
      provenance={node}
      id={node.id}
      actions={<Copy text={node.text} />}
    >
      <Prose text={node.text} kind="ask" />
    </Block>
  );
}

/**
 * The model on the trunk: what it read, apart from what it wrote. The
 * reading is its header, along its top edge -- right under the ask or tool
 * output it mostly is; the writing is its body, reasoning in italic then the
 * answer streamed from line 1, and its footer. Both count up while they run.
 * The tool calls it ended in (`calls`) are the nodes after it (ToolBlock):
 * then its footer is its text's share, where the drive said where the calls
 * began, and nothing where it did not -- the whole closes the first call.
 * Having written no text, it is one row: what it read.
 */
export function AssistantMessage({ node, calls = [] }: { readonly node: Folded<AssistantNode>; readonly calls?: readonly Folded<ToolNode>[] }) {
  const [thinking, setThinking] = useState(false);
  const live = node.progress === 'prefill' || node.progress === 'streaming';
  const long = node.reasoning.length > 280;
  const showReasoning = node.reasoning !== '' && (thinking || !long || (live && node.text === ''));
  const now = useNow();
  // Prefill is silent by nature, and a long one is the cost worth flagging: it
  // escalates on its total. A generation escalates only on silence -- time
  // since its last token -- never while tokens are arriving.
  const worry = node.progress === 'prefill' ? elapsed(now, node.startedAt).level : elapsed(now, node.lastActivityAt).level;
  const streamingInto = node.progress === 'streaming' ? (node.text === '' ? 'reasoning' : 'answer') : undefined;
  // It wrote no text: a step, not a message.
  const bare = node.progress === 'done' && node.text === '' && node.reasoning === '';
  const stopped = stopOf(node.stop ?? 'stop');
  const reading = readingOf(node, now);
  const apart = calls.length > 0 ? writtenApart(node) : undefined;
  // What it wrote, in its footer: all of it, or its text's share, or -- ending in calls it wrote as one with them -- nothing here.
  const writing = calls.length === 0 ? writingOf(node, now) : apart?.text;
  const warm = warmOf(node);
  const readLine = reading ? <Flowing flow={reading} level={worry} {...(warm !== undefined ? { title: `new tokens read; ${tokens(warm)} more were warm, reused from the slot` } : {})} /> : undefined;
  return (
    <Block
      tone="assistant"
      label="assistant"
      thin={bare}
      live={live}
      intake={{ reading: node.progress === 'prefill', edge: edgeOf(node) }}
      {...(readLine && !bare ? { input: readLine } : {})}
      {...(writing && !node.failure && !bare ? { output: <Flowing flow={writing} level={worry} title={apart ? 'tokens written before the tool calls began' : 'tokens written, reasoning included'} /> } : {})}
      alarm={node.failure ? 'bad' : alarmOf(stopped.level)}
      stats={[
        node.failure && {
          value: (
            <span className="ex-failure" data-level={failOf(node.failure.reason).level}>
              failed · {failOf(node.failure.reason).label}
              {node.wallMs !== undefined ? ` · ${took(node.wallMs)}` : ''}
            </span>
          ),
          title: node.failure.message,
        },
        !node.failure &&
          stopped.level !== 'ok' && {
            value: (
              <span className="ex-stop" data-level={stopped.level}>
                {stopped.label}
              </span>
            ),
            title: 'why generation stopped',
          },
      ]}
      provenance={node}
      id={node.id}
      {...(!live && node.text !== '' ? { actions: <Copy text={node.text} /> } : {})}
    >
      {bare ? readLine : <AssistantBody node={node} live={live} long={long} showReasoning={showReasoning} thinking={thinking} setThinking={setThinking} streamingInto={streamingInto} />}
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
        // What it reads is the header's; here is only where its first token will land.
        <p className="ex-waiting">
          <span className="ex-caret" aria-hidden="true" />
        </p>
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
      id={node.id}
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

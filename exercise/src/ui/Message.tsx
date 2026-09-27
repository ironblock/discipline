import { useState } from 'react';

import type { AssistantNode, Folded, SettledNode, SystemNode, ToolNode, UserNode } from '../session/fold.ts';
import { Block } from './Block.tsx';
import { tokens, took } from './format.ts';
import { Copy } from './Copy.tsx';
import { edgeOf, readingOf, warmOf, writingOf } from './flow.ts';
import { Flowing } from './Flowing.tsx';
import { Prose } from './Prose.tsx';
import { alarmOf, callOf, failOf, settleOf, stopOf } from './sets.ts';
import { CallCell } from './ToolCall.tsx';
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
      heads={[node.tokens !== undefined && { value: tokens(node.tokens), unit: 'tok', title: 'tokens in the prefix' }]}
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
      heads={
        node.prefill
          ? [
              {
                value: `+${tokens(node.prefill.fresh)}`,
                unit: 'tok',
                title: `new tokens this ask put in front of the model; ${tokens(node.prefill.cached)} before it were warm, reused from the slot`,
              },
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

/**
 * The model on the trunk: what it read, apart from what it wrote. The
 * reading is its header, along its top edge -- right under the ask or tool
 * output it mostly is; the writing is its body, reasoning in italic then the
 * answer streamed from line 1, then the tool call it ended in, if it did
 * (`call`: the model wrote it, so it is part of what came out); and its
 * footer, what it wrote in all. Both count up while they run.
 */
export function AssistantMessage({ node, call }: { readonly node: Folded<AssistantNode>; readonly call?: Folded<ToolNode> | undefined }) {
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
  // It said nothing, and its call has not arrived yet: a step, not a message.
  const bare = node.progress === 'done' && node.text === '' && node.reasoning === '' && !call;
  const stopped = stopOf(node.stop ?? 'stop');
  const reading = readingOf(node, now);
  const writing = writingOf(node, now);
  const warm = warmOf(node);
  const command = call ? callOf(call.tool, call.args) : undefined;
  return (
    <Block
      tone="assistant"
      label="assistant"
      thin={bare}
      live={live}
      intake={{
        reading: node.progress === 'prefill',
        edge: edgeOf(node),
        ...(reading ? { line: <Flowing flow={reading} level={worry} {...(warm !== undefined ? { title: `new tokens read; ${tokens(warm)} more were warm, reused from the slot` } : {})} /> } : {}),
      }}
      {...(writing && !node.failure ? { output: <Flowing flow={writing} level={worry} title="tokens written, reasoning and any tool call included" /> } : {})}
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
      {...(!live && (node.text !== '' || command)
        ? {
            actions: (
              <>
                {node.text !== '' ? <Copy text={node.text} /> : null}
                {command ? <Copy text={command.text} label={command.prompt === '$' ? 'copy command' : 'copy call'} /> : null}
              </>
            ),
          }
        : {})}
    >
      {bare ? undefined : (
        <>
          <AssistantBody node={node} live={live} long={long} showReasoning={showReasoning} thinking={thinking} setThinking={setThinking} streamingInto={streamingInto} />
          {call ? <CallCell node={call} /> : null}
        </>
      )}
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

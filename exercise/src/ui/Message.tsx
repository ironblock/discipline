import { useState } from 'react';

import type { AssistantNode, Folded, SystemNode, UserNode } from '../session/fold.ts';
import { Block } from './Block.tsx';
import { counter, ms, rate, tokens } from './format.ts';
import { Prose } from './Prose.tsx';
import { elapsed, useNow } from './surface.tsx';
import './message.css';

/** The trunk's system prompt: the phase's priming, or working memory rendered. */
export function SystemMessage({ node }: { readonly node: Folded<SystemNode> }) {
  const [open, setOpen] = useState(false);
  const lineCount = node.text.split('\n').length;
  const shown = open ? node.text : node.text.split('\n').slice(0, 3).join('\n');
  return (
    <Block
      tone="system"
      label={node.render === undefined ? 'system' : `system · render v${node.render}`}
      stats={[{ value: tokens(node.tokens), unit: 'tok', title: 'tokens in the prefix' }]}
      provenance={node}
      id={node.id}
    >
      <Prose text={shown} kind="system" />
      {lineCount > 3 ? (
        <button type="button" className="ex-more" onClick={() => setOpen(!open)}>
          {open ? 'less' : `${lineCount - 3} more lines`}
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
  return (
    <Block
      tone="assistant"
      label="assistant"
      live={live}
      stats={
        t
          ? [
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
    >
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
    </Block>
  );
}

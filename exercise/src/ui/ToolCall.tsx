import { useState } from 'react';

import type { Folded, ToolNode } from '../session/fold.ts';
import { Block } from './Block.tsx';
import { bytes, lines, took } from './format.ts';
import { Copy } from './Copy.tsx';
import { callOf } from './sets.ts';
import { elapsed, useNow } from './surface.tsx';
import './tool.css';

/** How much of a result shows before it is opened. */
const PEEK = 3;

/**
 * A tool call as the model wrote it: the end of an assistant message's body
 * (AssistantMessage's `call`), since its tokens are that generation's. The
 * tool's chip and its command; a script's further lines when opened. Its
 * result is the block below -- the two a pair, as a REPL's input and output.
 */
export function CallCell({ node }: { readonly node: Folded<ToolNode> }) {
  const [open, setOpen] = useState(false);
  const call = callOf(node.tool, node.args);
  const [first, ...rest] = call.text.split('\n');
  return (
    <div className="ex-call" data-tool={node.tool}>
      <span className="ex-call__label">{call.label}</span>
      <button type="button" className="ex-tool__head" aria-expanded={open} onClick={() => setOpen(!open)} disabled={rest.length === 0}>
        <span className="ex-tool__caret" aria-hidden="true">
          {open ? '▾' : '▸'}
        </span>
        {call.prompt ? <span className="ex-tool__prompt">{call.prompt}</span> : null}
        <span className="ex-tool__command">{first}</span>
        {rest.length > 0 && !open ? <span className="ex-tool__more">+{rest.length} lines</span> : null}
      </button>
      {open && rest.length > 0 ? <pre className="ex-tool__script">{rest.join('\n')}</pre> : null}
    </div>
  );
}

/**
 * What a tool call returned: its own block, under the call, with the tool's
 * stats rather than tokens -- lines and bytes and how long, its exit. Its
 * first lines show; the rest when opened. Running, or having printed
 * nothing, it is one row.
 */
export function ToolResult({ node, open: initiallyOpen = false }: { readonly node: Folded<ToolNode>; readonly open?: boolean }) {
  const [open, setOpen] = useState(initiallyOpen);
  const since = elapsed(useNow(), node.startedAt);
  const output = node.output ?? '';
  const all = output === '' ? [] : output.split('\n');
  const more = all.length > PEEK;
  const row = node.running || output === '';
  const exit = node.exit !== undefined && {
    value: <span className={node.exit === 0 ? 'ex-exit ex-exit--ok' : 'ex-exit ex-exit--bad'}>exit {node.exit}</span>,
    title: 'the exit status',
  };
  return (
    <Block
      tone="tool"
      label="result"
      thin={row}
      live={node.running}
      alarm={node.exit !== undefined && node.exit !== 0 ? 'bad' : undefined}
      {...(!row && more
        ? {
            lead: (
              <button type="button" className="ex-tool__head" aria-expanded={open} onClick={() => setOpen(!open)}>
                <span className="ex-tool__caret" aria-hidden="true">
                  {open ? '▾' : '▸'}
                </span>
                <span className="ex-tool__more">{open ? 'all' : 'first'} {open ? all.length.toLocaleString('en-US') : PEEK} of {all.length.toLocaleString('en-US')} lines</span>
              </button>
            ),
          }
        : {})}
      output={
        node.running ? (
          <span className="ex-elapsed" data-level={since.level}>
            running · {took(since.ms)}
          </span>
        ) : (
          `${output === '' ? 'no output' : `${lines(output).toLocaleString('en-US')} lines · ${bytes(output)}`} in ${took(node.ms ?? 0)}`
        )
      }
      stats={[node.truncated && { value: <span className="ex-truncated">truncated</span>, title: 'the harness cut the output before the model saw it' }, exit]}
      provenance={node}
      id={node.id}
      {...(output !== '' ? { actions: <Copy text={output} label="copy output" /> } : {})}
    >
      {row ? undefined : <pre className="ex-tool__output">{open || !more ? output : all.slice(0, PEEK).join('\n')}</pre>}
    </Block>
  );
}

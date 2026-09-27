import { useState } from 'react';

import type { AssistantNode, Folded, ToolNode } from '../session/fold.ts';
import { Block } from './Block.tsx';
import { bytes, lines, took } from './format.ts';
import { Copy } from './Copy.tsx';
import { callOf } from './sets.ts';
import { writingOf, writtenApart } from './flow.ts';
import { Flowing } from './Flowing.tsx';
import { elapsed, useNow } from './surface.tsx';
import './tool.css';

/** How much of a result shows before it is opened. */
const PEEK = 3;

/**
 * A tool call, and what it returned: a pair, like a REPL's input and output,
 * drawn as one node of the trunk. The call is what the model wrote after its
 * message -- the tool's chip and its command; a script's further lines when
 * opened -- and its footer is what writing it took, where the drive said
 * where the calls began (`callsFrom`). Where it did not, the message and its
 * calls were written as one and only the whole is known: it closes the last
 * thing written, the first call, marked as both. The result follows.
 */
export function ToolPair({ node, caller, first }: { readonly node: Folded<ToolNode>; readonly caller?: Folded<AssistantNode> | undefined; readonly first: boolean }) {
  const [open, setOpen] = useState(false);
  const call = callOf(node.tool, node.args);
  const [line, ...rest] = call.text.split('\n');
  const apart = caller ? writtenApart(caller) : undefined;
  const whole = caller && !apart ? writingOf(caller, 0) : undefined;
  return (
    <div className="ex-pair">
      <Block
        tone="tool"
        label={call.label}
        lead={
          <button type="button" className="ex-tool__head" aria-expanded={open} onClick={() => setOpen(!open)} disabled={rest.length === 0}>
            <span className="ex-tool__caret" aria-hidden="true">
              {open ? '▾' : '▸'}
            </span>
            {call.prompt ? <span className="ex-tool__prompt">{call.prompt}</span> : null}
            <span className="ex-tool__command">{line}</span>
            {rest.length > 0 && !open ? <span className="ex-tool__more">+{rest.length} lines</span> : null}
          </button>
        }
        {...(first && apart ? { output: <Flowing flow={apart.calls} title="tokens written for the tool calls" /> } : {})}
        {...(first && whole ? { output: <Flowing flow={whole} title="tokens written, the message above and its calls together: the drive did not say where the calls began" /> } : {})}
        stats={[first && whole && { value: <span className="ex-pair__both">message and call</span>, title: 'written as one: the drive did not say where the calls began' }]}
        provenance={node}
        actions={<Copy text={call.text} label={call.prompt === '$' ? 'copy command' : 'copy call'} />}
      >
        {open && rest.length > 0 ? <pre className="ex-tool__script">{rest.join('\n')}</pre> : undefined}
      </Block>
      <ToolResult node={node} />
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

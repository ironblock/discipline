import { useMemo, useState } from 'react';

import type { AssistantNode, Folded, ToolNode } from '../session/fold.ts';
import { Block } from './Block.tsx';
import { bytes, count, lines, took } from './format.ts';
import { Copy } from './Copy.tsx';
import { callOf } from './sets.ts';
import { writingOf, writtenApart } from './flow.ts';
import { Flowing } from './Flowing.tsx';
import { elapsed, useNow } from './surface.tsx';
import './tool.css';

/** How much of a script, and of what it printed, shows before the block is opened. */
const PEEK = 3;

/**
 * A tool call, one block of the trunk, set as a message is: its header what
 * went in, its footer what came out. What went in is what the model wrote
 * -- an in-turn step, so its tokens written are the call's input: where the
 * drive said where the calls began (`callsFrom`), the calls' share, on the
 * first call of a step; where it did not, the message and its calls were
 * written as one and only the whole is known, so the first call carries the
 * whole, marked as both. Its body reads like a REPL: the command, and what
 * it printed under it, their first lines until opened. What came out is the
 * tool's stats rather than tokens -- lines and bytes and how long, its exit.
 */
export function ToolBlock({ node, caller, first = false }: { readonly node: Folded<ToolNode>; readonly caller?: Folded<AssistantNode> | undefined; readonly first?: boolean }) {
  const [open, setOpen] = useState(false);
  const since = elapsed(useNow(), node.startedAt);
  const call = callOf(node.tool, node.args);
  const script = call.text.split('\n');
  const output = node.output ?? '';
  // What it printed, taken apart once per output, not once per render: every block re-renders on every event
  // of a session, and splitting and encoding a long output each time was most of a replay's script (scripts/perf.mjs).
  const printed = useMemo(() => (output === '' ? [] : output.split('\n')), [output]);
  const said = useMemo(() => (output === '' ? 'no output' : `${count(lines(output))} ${lines(output) === 1 ? 'line' : 'lines'} · ${bytes(output)}`), [output]);
  const hidden = Math.max(0, script.length - PEEK) + Math.max(0, printed.length - PEEK);
  const apart = first && caller ? writtenApart(caller) : undefined;
  const whole = first && caller && !apart ? writingOf(caller, 0) : undefined;
  const exit = node.exit !== undefined && {
    value: <span className={node.exit === 0 ? 'ex-exit ex-exit--ok' : 'ex-exit ex-exit--bad'}>exit {node.exit}</span>,
    title: 'the exit status',
  };
  return (
    <Block
      tone="tool"
      label={call.label}
      {...(apart ? { input: <Flowing flow={apart.calls} title="tokens written for the tool calls" /> } : {})}
      {...(whole
        ? {
            input: (
              <>
                <Flowing flow={whole} title="tokens written, the message above and its calls together: the drive did not say where the calls began" />
                <span className="ex-tool__both" title="written as one: the drive did not say where the calls began">
                  {' · message and call'}
                </span>
              </>
            ),
          }
        : {})}
      live={node.running}
      alarm={node.exit !== undefined && node.exit !== 0 ? 'bad' : undefined}
      output={
        node.running ? (
          <span className="ex-elapsed" data-level={since.level}>
            running · {took(since.ms)}
          </span>
        ) : (
          `${said} in ${took(node.ms ?? 0)}`
        )
      }
      stats={[node.truncated && { value: <span className="ex-truncated">truncated</span>, title: 'the harness cut the output before the model saw it' }, exit]}
      provenance={node}
      id={node.id}
      actions={
        <>
          <Copy text={call.text} label={call.prompt === '$' ? 'copy command' : 'copy call'} />
          {output !== '' ? <Copy text={output} label="copy output" /> : null}
        </>
      }
    >
      <pre className="ex-tool__call">
        {call.prompt ? <span className="ex-tool__prompt">{call.prompt} </span> : null}
        {open ? call.text : script.slice(0, PEEK).join('\n')}
        {!open && script.length > PEEK ? <span className="ex-tool__clip">{'\n'}…</span> : null}
      </pre>
      {printed.length > 0 ? <pre className="ex-tool__output">{open ? output : printed.slice(0, PEEK).join('\n')}</pre> : null}
      {hidden > 0 ? (
        <button type="button" className="ex-more" aria-expanded={open} onClick={() => setOpen(!open)}>
          {open ? 'less' : `${count(hidden)} more ${hidden === 1 ? 'line' : 'lines'}`}
        </button>
      ) : null}
    </Block>
  );
}

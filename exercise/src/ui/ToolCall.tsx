import { useState } from 'react';

import type { Folded, ToolNode } from '../session/fold.ts';
import { Block } from './Block.tsx';
import { bytes, lines, ms } from './format.ts';
import './tool.css';

/** A bash call on the trunk: the command, then its output, collapsed until asked for. */
export function ToolCall({ node, open: initiallyOpen = false }: { readonly node: Folded<ToolNode>; readonly open?: boolean }) {
  const [open, setOpen] = useState(initiallyOpen);
  const [first, ...rest] = node.command.split('\n');
  const output = node.output ?? '';
  return (
    <Block
      tone="tool"
      label="bash"
      live={node.running}
      stats={
        node.running
          ? [{ value: 'running…' }]
          : [
              node.ms !== undefined && { value: ms(node.ms), title: 'wall clock' },
              { value: output === '' ? 'no output' : `${lines(output).toLocaleString('en-US')} lines · ${bytes(output)}`, title: 'what it printed' },
              { value: <span className={node.exit === 0 ? 'ex-exit ex-exit--ok' : 'ex-exit ex-exit--bad'}>exit {node.exit}</span> },
            ]
      }
      provenance={node}
      id={node.id}
    >
      <button type="button" className="ex-tool__head" aria-expanded={open} onClick={() => setOpen(!open)} disabled={output === '' && rest.length === 0}>
        <span className="ex-tool__caret" aria-hidden="true">
          {open ? '▾' : '▸'}
        </span>
        <span className="ex-tool__prompt">$</span>
        <span className="ex-tool__command">{first}</span>
        {rest.length > 0 && !open ? <span className="ex-tool__more">+{rest.length} lines</span> : null}
      </button>
      {open ? (
        <>
          {rest.length > 0 ? <pre className="ex-tool__script">{rest.join('\n')}</pre> : null}
          {output !== '' ? <pre className="ex-tool__output">{output}</pre> : null}
        </>
      ) : null}
    </Block>
  );
}

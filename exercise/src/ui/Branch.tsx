import { useState } from 'react';

import type { BranchNode, Folded, PatchNode } from '../session/fold.ts';
import { Block } from './Block.tsx';
import { ms, rate, tokens } from './format.ts';
import { Prose } from './Prose.tsx';
import './branch.css';

const OP_GLYPH = { add: '+', supersede: '↻', retire: '−' } as const;

/**
 * A side call off the trunk's warm tail, in the slot that served it: a thin
 * bar saying what `diet` noticed and asked, the answer when opened, and the
 * patches it landed in working memory.
 */
export function Branch({ node, open: initiallyOpen = false }: { readonly node: Folded<BranchNode>; readonly open?: boolean }) {
  const [open, setOpen] = useState(initiallyOpen);
  const live = node.outcome === undefined;
  const t = node.timings;
  return (
    <div className="ex-branch" data-lane={node.lane} data-outcome={node.outcome ?? 'running'}>
      <Block
        tone={node.lane}
        label={node.lane}
        thin
        live={live}
        stats={[
          node.wallMs !== undefined && { value: ms(node.wallMs), title: 'wall clock' },
          t && { value: tokens(t.predicted_n), unit: 'tok', title: 'tokens generated' },
          t && { value: tokens(t.prompt_n), unit: 'new', title: 'prompt tokens evaluated: the question alone, if the fork was warm' },
          t && { value: tokens(t.cache_n), unit: 'warm', title: 'prompt tokens reused from the trunk’s tail' },
          t && { value: rate(t.predicted_n, t.predicted_ms), unit: 't/s' },
          node.outcome !== undefined && node.outcome !== 'complete' && { value: <span className="ex-branch__outcome">{node.outcome}</span> },
        ]}
        provenance={node}
        id={node.id}
      />
      <button type="button" className="ex-branch__why" aria-expanded={open} onClick={() => setOpen(!open)}>
        <span className="ex-branch__caret" aria-hidden="true">
          {open ? '▾' : '▸'}
        </span>
        {node.why}
      </button>
      {open ? (
        <div className="ex-branch__exchange">
          <p className="ex-branch__question">{node.question}</p>
          {node.text !== undefined && node.text !== '' ? (
            <div className="ex-branch__answer">
              <Prose text={node.text} kind="answer" />
            </div>
          ) : (
            <p className="ex-branch__waiting">{node.progress === 'prefill' ? 'prefill…' : 'generating…'}</p>
          )}
        </div>
      ) : null}
      {node.patches.length > 0 ? (
        <ul className="ex-branch__patches">
          {node.patches.map((p) => (
            <PatchLine key={p.id} patch={p} />
          ))}
        </ul>
      ) : null}
    </div>
  );
}

function PatchLine({ patch }: { readonly patch: Folded<PatchNode> }) {
  return (
    <li className="ex-patch" data-op={patch.op} data-from={patch.from.join(' ')} data-needs={patch.needs.join(' ')}>
      <span className="ex-patch__op" aria-label={patch.op}>
        {OP_GLYPH[patch.op]}
      </span>
      <span className="ex-patch__id">#{patch.entryId}</span>
      <span className="ex-patch__text">{patch.text}</span>
      {patch.supersedes ? <span className="ex-patch__note">replaces #{patch.supersedes}</span> : null}
    </li>
  );
}

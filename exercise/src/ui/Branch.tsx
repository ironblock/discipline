import { useState } from 'react';

import type { BranchNode, Folded, PatchNode } from '../session/fold.ts';
import { Block } from './Block.tsx';
import { barHeight, rowsOf } from './condensed.ts';
import { ms, rate, tokens } from './format.ts';
import { alarmOf, failOf, opOf, outcomeOf } from './sets.ts';
import './branch.css';

/** Patches a side call shows before "N more": enough to see what it did, few enough that side calls stay level with the trunk. */
const PATCHES_SHOWN = 3;

/**
 * A side call off the trunk's warm tail, in the slot that served it: a thin
 * bar saying what `diet` noticed and asked, the answer when opened, and the
 * patches it landed in working memory.
 */
export function Branch({ node, open: initiallyOpen = false }: { readonly node: Folded<BranchNode>; readonly open?: boolean }) {
  const [open, setOpen] = useState(initiallyOpen);
  const [allPatches, setAllPatches] = useState(false);
  const pending = isPending(node);
  const live = node.outcome === undefined && !pending;
  const outcome = node.outcome !== undefined ? outcomeOf(node.outcome) : undefined;
  const t = node.timings;
  return (
    <div className="ex-branch" data-lane={node.lane} data-outcome={node.outcome ?? (pending ? 'pending' : 'running')}>
      <Block
        tone="lane"
        lane={node.lane}
        label={node.lane}
        thin
        live={live}
        alarm={outcome ? alarmOf(outcome.level) : undefined}
        stats={[
          node.wallMs !== undefined && { value: ms(node.wallMs), title: 'wall clock' },
          t && { value: tokens(t.predicted_n), unit: 'tok', title: 'tokens generated' },
          t && { value: tokens(t.prompt_n), unit: 'new', title: 'prompt tokens evaluated: the question alone, if the fork was warm' },
          t && { value: tokens(t.cache_n), unit: 'warm', title: 'prompt tokens reused from the trunk’s tail' },
          t && { value: rate(t.predicted_n, t.predicted_ms), unit: 't/s' },
          pending && {
            value: (
              <span className="ex-branch__outcome" data-level="quiet" data-known="">
                queued
              </span>
            ),
            title: `declared, waiting for slot ${node.slot}`,
          },
          outcome !== undefined &&
            node.outcome !== 'complete' && {
              value: (
                <span className="ex-branch__outcome" data-level={outcome.level} data-known={outcome.known ? '' : undefined}>
                  {outcome.label}
                </span>
              ),
              title: outcome.known ? 'how the side call ended' : 'how the side call ended: a name this surface does not know yet',
            },
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
              <TaggedAnswer text={node.text} />
            </div>
          ) : node.failure ? (
            <p className="ex-branch__failed">
              {failOf(node.failure.reason).label}: {node.failure.message}
            </p>
          ) : (
            <p className="ex-branch__waiting">{pending ? `waiting for slot ${node.slot}…` : node.progress === 'prefill' ? 'prefill…' : 'generating…'}</p>
          )}
        </div>
      ) : null}
      {node.patches.length > 0 ? (
        <ul className="ex-branch__patches">
          {(allPatches ? node.patches : node.patches.slice(0, PATCHES_SHOWN)).map((p) => (
            <PatchLine key={p.id} patch={p} />
          ))}
          {node.patches.length > PATCHES_SHOWN ? (
            <li>
              <button type="button" className="ex-more" aria-expanded={allPatches} onClick={() => setAllPatches(!allPatches)}>
                {allPatches ? 'fewer' : `${node.patches.length - PATCHES_SHOWN} more patches`}
              </button>
            </li>
          ) : null}
        </ul>
      ) : null}
    </div>
  );
}

/**
 * A side call condensed: a bar in its lane's colour that keeps its place
 * beside the trunk, as long as what it has written (a row at a time, as it
 * streams) with a tick per patch it landed, in the op's colour. Running, it
 * is lit, and its leading edge is where the writing is; an outcome worth
 * seeing is a tick across its top. Pressing it opens the side call.
 */
export function BranchBar({ node, onOpen }: { readonly node: Folded<BranchNode>; readonly onOpen?: () => void }) {
  const pending = isPending(node);
  const live = node.outcome === undefined && !pending;
  const outcome = node.outcome !== undefined ? outcomeOf(node.outcome) : undefined;
  const alarm = outcome ? alarmOf(outcome.level) : undefined;
  const patches = node.patches.length;
  const state = outcome ? outcome.label : pending ? 'queued' : node.progress === 'prefill' ? 'prefill' : 'writing';
  const label = `${node.lane} ${node.id}: ${node.why} · ${state} · ${patches} patch${patches === 1 ? '' : 'es'}`;
  return (
    <button
      type="button"
      className="ex-bar"
      data-lane={node.lane}
      data-live={live ? '' : undefined}
      data-pending={pending ? '' : undefined}
      data-progress={live ? node.progress : undefined}
      data-alarm={alarm}
      data-from={node.from.join(' ')}
      data-needs={node.needs.join(' ')}
      aria-label={label}
      title={label}
      style={{ height: barHeight(rowsOf(node.text), patches) }}
      onClick={onOpen}
    >
      {node.patches.map((p) => (
        <span key={p.id} className="ex-bar__tick" data-level={opOf(p.op).level} />
      ))}
    </button>
  );
}

/** Declared but not started: a fork whose request has not gone to its slot yet. */
function isPending(node: BranchNode): boolean {
  return node.startedAt === undefined && node.outcome === undefined;
}

/**
 * An interview answer in its grammar: `TAG: text` lines, the tag set apart
 * in the harness's face so the answer scans as the record it is. Lines
 * without a tag are left as prose. Display only: the grammar is parsed by
 * `diet`, and nothing here decides what a tag means.
 */
function TaggedAnswer({ text }: { readonly text: string }) {
  return (
    <div className="ex-tagged">
      {text.split('\n').map((line, i) => {
        const m = /^([A-Z][A-Z_]+):\s?(.*)$/.exec(line);
        return m ? (
          <p key={i} className="ex-tagged__line">
            <span className="ex-tagged__tag">{m[1]}</span>
            <span className="ex-tagged__text">{m[2]}</span>
          </p>
        ) : (
          <p key={i} className="ex-tagged__line ex-tagged__line--plain">
            {line}
          </p>
        );
      })}
    </div>
  );
}

function PatchLine({ patch }: { readonly patch: Folded<PatchNode> }) {
  const op = opOf(patch.op);
  return (
    <li
      className="ex-patch"
      data-op={patch.op}
      data-level={op.level}
      data-known={op.known ? '' : undefined}
      data-from={patch.from.join(' ')}
      data-needs={patch.needs.join(' ')}
      title={patch.provenance ? `${op.label} · ${patch.provenance}` : op.label}
    >
      <span className="ex-patch__op" aria-label={op.label} title={op.label}>
        {op.glyph}
      </span>
      <span className="ex-patch__id">#{patch.entryId}</span>
      <span className="ex-patch__text" title={patch.text}>
        {patch.text}
      </span>
      {!op.known ? <span className="ex-patch__note">{op.label}</span> : null}
      {patch.supersedes ? <span className="ex-patch__note">replaces #{patch.supersedes}</span> : null}
    </li>
  );
}

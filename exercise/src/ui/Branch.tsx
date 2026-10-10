import { useState } from 'react';

import type { BranchNode, Folded, PatchNode } from '../session/fold.ts';
import { Block } from './Block.tsx';
import { barHeight, rowsOf } from './condensed.ts';
import { edgeOf, readingOf, warmOf, writingOf } from './flow.ts';
import { Flowing } from './Flowing.tsx';
import { useNow } from './surface.tsx';
import { took, tokens } from './format.ts';
import { alarmOf, failOf, opOf, outcomeOf } from './sets.ts';
import './branch.css';

/**
 * A side call off the trunk's warm tail, in the slot that served it: a thin
 * bar saying what it wrote and how many patches of each op it landed --
 * what they say is in working memory, a line away (Links.tsx) -- and under
 * it what `diet` noticed. Opened, it is a block as the trunk's are: its
 * header what it read, its body the question, the answer and the patches,
 * its footer what it wrote.
 */
export function Branch({ node, open: initiallyOpen = false }: { readonly node: Folded<BranchNode>; readonly open?: boolean }) {
  const [open, setOpen] = useState(initiallyOpen);
  const pending = isPending(node);
  const live = node.outcome === undefined && !pending;
  const outcome = node.outcome !== undefined ? outcomeOf(node.outcome) : undefined;
  const now = useNow();
  // What it read: along its bar's top edge, and its header opened; what it wrote, on its bar and its footer opened.
  const reading = pending ? undefined : readingOf(node, now);
  const writing = pending ? undefined : writingOf(node, now);
  const warm = warmOf(node);
  return (
    <div className="ex-branch" data-lane={node.lane} data-outcome={node.outcome ?? (pending ? 'pending' : 'running')}>
      <Block
        tone="lane"
        lane={node.lane}
        label={node.lane}
        thin={!open}
        live={live}
        {...(reading ? { intake: { reading: reading.running, edge: edgeOf(node) } } : {})}
        {...(open && reading ? { input: <Flowing flow={reading} {...(warm !== undefined ? { title: `new tokens read; ${tokens(warm)} more were warm, the trunk's prefix` } : {})} /> } : {})}
        {...(writing ? { output: <Flowing flow={writing} title="tokens written" /> } : {})}
        alarm={outcome ? alarmOf(outcome.level) : undefined}
        stats={[
          // An offboard seat (#615): where it ran, and what reading the trunk cold cost there. A warm fork names none.
          node.seat && {
            value: (
              <span className="ex-branch__seat">
                {[node.seat.substrate, ...(node.seat.promptTokens !== undefined ? [`${tokens(node.seat.promptTokens)} tok`] : []), ...(node.seat.wallMs !== undefined ? [took(node.seat.wallMs)] : [])].join(' · ')}
              </span>
            ),
            title: `ran offboard on ${node.seat.substrate} (${node.seat.model}), reading the trunk cold`,
          },
          node.patches.length > 0 && { value: <PatchSummary patches={node.patches} />, title: 'patches it landed in working memory, by op' },
          pending && {
            value: (
              <span className="ex-branch__outcome" data-level="quiet" data-known="">
                queued
              </span>
            ),
            title: `declared, waiting for slot ${node.slot}`,
          },
          outcome !== undefined &&
            node.outcome !== 'value' && {
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
      >
        {open ? (
          <div className="ex-branch__exchange">
            <p className="ex-branch__question">{node.question}</p>
            {node.text !== undefined && node.text !== '' ? <TaggedAnswer text={node.text} /> : null}
            {node.failure ? (
              // What failed is the footer's to name; here, what the server said, under what it had written.
              <p className="ex-failed" role="alert" title={failOf(node.failure.reason).label}>
                {node.failure.message}
              </p>
            ) : null}
            {(node.text === undefined || node.text === '') && !node.failure && node.outcome === undefined ? (
              <p className="ex-branch__waiting">{pending ? `waiting for slot ${node.slot}…` : node.progress === 'prefill' ? 'reading…' : 'writing…'}</p>
            ) : null}
            {node.patches.length > 0 ? (
              <ul className="ex-branch__patches">
                {node.patches.map((p) => (
                  <PatchLine key={p.id} patch={p} />
                ))}
              </ul>
            ) : null}
          </div>
        ) : undefined}
      </Block>
      <button type="button" className="ex-branch__why" aria-expanded={open} onClick={() => setOpen(!open)}>
        <span className="ex-branch__caret" aria-hidden="true">
          {open ? '▾' : '▸'}
        </span>
        {node.why}
      </button>
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

/** How many patches of each op, in the op's glyph and colour: `+3 ↻1 −1`. */
function PatchSummary({ patches }: { readonly patches: readonly Folded<PatchNode>[] }) {
  const counts = new Map<string, number>();
  for (const p of patches) counts.set(p.op, (counts.get(p.op) ?? 0) + 1);
  return (
    <span className="ex-patchsum">
      {[...counts].map(([op, n]) => {
        const drawn = opOf(op);
        return (
          <span key={op} className="ex-patchsum__op" data-level={drawn.level} data-known={drawn.known ? '' : undefined} title={drawn.label}>
            {drawn.glyph}
            {n}
          </span>
        );
      })}
    </span>
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
      title={patch.authority ? `${op.label} · ${patch.authority}` : op.label}
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

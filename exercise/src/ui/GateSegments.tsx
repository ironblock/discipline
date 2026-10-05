import { useEffect, useState } from 'react';

import { judge } from '../gate/judge.ts';
import type { Judgement } from '../gate/judge.ts';
import type { Segment } from '../drive/transport.ts';

/**
 * A command's segments as the gate read it (#389): one row each, what an
 * approval would cover, what became of it and why. The live prompt draws the
 * waiting event's segments with it; a replay draws the ones `JudgedSegments`
 * re-derives from the logged `argv` -- the same rows from the same reader.
 */
export function SegmentList({ segments }: { readonly segments: readonly Segment[] }) {
  return (
    <ol className="ex-approval__segments" aria-label="the command, as the gate read it">
      {segments.map((segment, i) => (
        <li key={i} className="ex-approval__segment" data-verdict={segment.verdict}>
          <code>{segment.shape ?? segment.entry ?? 'no shape'}</code>
          <span className="ex-approval__verdict">
            {segment.verdict}
            {segment.why !== undefined ? ` · ${segment.why}` : ''}
            {segment.reason !== undefined ? `: ${segment.reason}` : ''}
            {segment.verdict === 'refused' && segment.entry !== undefined ? ` · on the denylist as ${segment.entry}` : ''}
          </span>
        </li>
      ))}
    </ol>
  );
}

type Judged = { readonly kind: 'judging' } | { readonly kind: 'judged'; readonly judgement: Judgement } | { readonly kind: 'unjudged'; readonly why: string };

/** A logged call's segments, re-derived from its `argv` by the gate (`gate/judge.ts`), never from its text. */
export function JudgedSegments({ argv }: { readonly argv: readonly string[] }) {
  const [judged, setJudged] = useState<Judged>({ kind: 'judging' });
  useEffect(() => {
    let live = true;
    setJudged({ kind: 'judging' });
    judge(argv).then(
      (said) => live && setJudged(said.ok ? { kind: 'judged', judgement: said.judgement } : { kind: 'unjudged', why: said.error }),
      (error: unknown) => live && setJudged({ kind: 'unjudged', why: String(error) }),
    );
    return () => {
      live = false;
    };
  }, [argv]);
  if (judged.kind !== 'judged') return <div className="ex-tool__held" data-judged={judged.kind}>{judged.kind === 'judging' ? 'reading the command…' : `the gate could not read it: ${judged.why}`}</div>;
  const segments = judged.judgement.segments;
  return <SegmentList segments={segments} />;
}

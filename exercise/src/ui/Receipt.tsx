import type { Receipt as Numbers } from '../session/receipt.ts';
import './panel.css';
import './receipt.css';

/**
 * The receipt: the six numbers #31 measures a session on, beside working
 * memory, as the session goes. The predecessor's first drive is the floor
 * they are read against. Each says on hover what it counts.
 */
export function Receipt({ receipt: r }: { readonly receipt: Numbers }) {
  const perAsk = (n: number) => (r.asks === 0 ? '–' : (n / r.asks).toFixed(1));
  const rows = [
    { measure: 'side-calls-per-ask', value: perAsk(r.sideCalls), label: 'side calls / ask', title: `${r.sideCalls} side calls for ${r.asks} asks` },
    { measure: 'patches-per-ask', value: perAsk(r.patches), label: 'patches / ask', title: `${r.patches} patches, one per entry touched, for ${r.asks} asks` },
    { measure: 'live-entries', value: String(r.liveEntries), label: 'live entries', title: 'entries in working memory now, neither superseded nor retired' },
    {
      measure: 'mimicry',
      // Unmeasured until a driven session under native tool calls (ruled on #25, 2026-09-28): no log this surface
      // reads types it yet -- the predecessor's count is a migration's heuristic, the rest are authored.
      value: 'unmeasured (confounded)',
      label: 'mimicry',
      title: `side calls that answered as the agent: ${r.mimicry} typed so in this log, and unmeasured until the first driven session under native tool calls. The floor's count typed a fenced bash block in an answer as mimicry, when a fenced block was the tool call -- it measured the tool convention (#25).`,
    },
    {
      measure: 'idle-before-refill',
      value: r.idleBeforeRefill.length === 0 ? '–' : r.idleBeforeRefill.map((ms) => `${Math.round(ms / 1000)} s`).join(' · '),
      label: 'trunk idle before refill',
      title: 'for each refill, from the trunk last finishing something to the seam',
    },
    {
      measure: 'side-call-time-in-gap',
      // Exact when every gap was measured (idle.gap, Q4); an upper bound, and said so, while any was not.
      value: r.sideCallMs === 0 ? '–' : `${r.gapsMeasured < r.gapsTotal ? '≤ ' : ''}${Math.round((100 * r.inGapMs) / r.sideCallMs)}%`,
      label: 'side-call time in a gap',
      title:
        r.gapsMeasured < r.gapsTotal
          ? `side-call time inside a person’s gap. ${r.gapsTotal - r.gapsMeasured} of ${r.gapsTotal} gaps were not measured, and count whole -- the turn handed back to the next ask or refill -- so this is an upper bound.`
          : 'side-call time inside the attended part of each gap -- noticing, reading, composing -- as the surface measured it: not time blocked on work in flight, nor away.',
    },
  ];
  return (
    <section className="ex-panel ex-receipt" aria-label="receipt">
      <header className="ex-receipt__head">receipt</header>
      <dl>
        {rows.map((row) => (
          <div key={row.measure} className="ex-receipt__row" data-measure={row.measure} title={row.title}>
            <dt>{row.label}</dt>
            <dd className="ex-receipt__value">{row.value}</dd>
          </div>
        ))}
      </dl>
    </section>
  );
}

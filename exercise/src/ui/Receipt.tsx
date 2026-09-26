import type { Receipt as Numbers } from '../session/receipt.ts';
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
    { measure: 'mimicry', value: String(r.mimicry), label: 'mimicry', title: 'side calls that answered as the agent' },
    {
      measure: 'idle-before-refill',
      value: r.idleBeforeRefill.length === 0 ? '–' : r.idleBeforeRefill.map((ms) => `${Math.round(ms / 1000)} s`).join(' · '),
      label: 'trunk idle before refill',
      title: 'for each refill, from the trunk last finishing something to the seam',
    },
    {
      measure: 'side-call-time-in-gap',
      // An upper bound until the log can tell reading from waiting: it says so.
      value: r.sideCallMs === 0 ? '–' : `≤ ${Math.round((100 * r.inGapMs) / r.sideCallMs)}%`,
      label: 'side-call time in a gap',
      title:
        'side-call time inside a person’s gap: from the trunk handing the turn back to their next ask or refill. The log cannot yet tell reading from waiting on capture, so this is an upper bound until the surface records idle gaps.',
    },
  ];
  return (
    <section className="ex-receipt" aria-label="receipt">
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

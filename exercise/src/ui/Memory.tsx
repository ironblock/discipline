import type { Folded, MemoryEntry } from '../session/fold.ts';
import './memory.css';

export interface MemoryProps {
  readonly entries: readonly Folded<MemoryEntry>[];
  /** Log position through which the person has acknowledged what landed. */
  readonly seenThrough?: number;
  /** Acknowledge everything that has landed so far. */
  readonly onSeen?: () => void;
}

/** Landed since the last ask, and not yet acknowledged. */
export function isUnseen(entry: MemoryEntry, seenThrough = -1): boolean {
  return entry.fresh && entry.state === 'live' && entry.landedAt > seenThrough;
}

/**
 * Working memory: the curated object that replaces "summarize this session".
 * Entries by category, what landed since the last ask marked fresh, and
 * what was superseded or retired kept visible, struck -- evicted from the
 * next render, never deleted.
 */
export function Memory({ entries, seenThrough = -1, onSeen }: MemoryProps) {
  const live = entries.filter((e) => e.state === 'live').length;
  const fresh = entries.filter((e) => isUnseen(e, seenThrough)).length;
  const categories = [...new Set(entries.map((e) => e.category))];
  return (
    <section className="ex-memory" aria-label="working memory">
      <header className="ex-memory__head">
        <span className="ex-memory__title">working memory</span>
        <span>{live} live</span>
        {fresh > 0 ? <span className="ex-memory__fresh">+{fresh} new</span> : null}
        {fresh > 0 && onSeen ? (
          <button type="button" className="ex-memory__seen" onClick={onSeen} title="mark what landed as seen">
            seen
          </button>
        ) : null}
      </header>
      {entries.length === 0 ? <p className="ex-memory__empty">Nothing yet. Interviews write here as the session goes.</p> : null}
      {categories.map((category) => (
        <div className="ex-memory__category" key={category}>
          <h3>{category}</h3>
          <ul>
            {entries
              .filter((e) => e.category === category)
              .map((e) => (
                <li
                  key={e.id}
                  className="ex-memory__entry"
                  data-state={e.state}
                  data-fresh={isUnseen(e, seenThrough) ? '' : undefined}
                  data-from={e.from.join(' ')}
                  data-needs={e.needs.join(' ')}
                  title={`${e.state} · last changed by ${e.by}`}
                >
                  <span className="ex-memory__id">#{e.id}</span>
                  <span className="ex-memory__text">{e.text}</span>
                </li>
              ))}
          </ul>
        </div>
      ))}
    </section>
  );
}

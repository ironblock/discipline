import type { Folded, MemoryEntry } from '../session/fold.ts';
import './memory.css';

/**
 * Working memory: the curated object that replaces "summarize this session".
 * Entries by category, what landed since the last ask marked fresh, and
 * what was superseded or retired kept visible, struck -- evicted from the
 * next render, never deleted.
 */
export function Memory({ entries }: { readonly entries: readonly Folded<MemoryEntry>[] }) {
  const live = entries.filter((e) => e.state === 'live').length;
  const fresh = entries.filter((e) => e.fresh && e.state === 'live').length;
  const categories = [...new Set(entries.map((e) => e.category))];
  return (
    <section className="ex-memory" aria-label="working memory">
      <header className="ex-memory__head">
        <span className="ex-memory__title">working memory</span>
        <span>{live} live</span>
        {fresh > 0 ? <span className="ex-memory__fresh">+{fresh} new</span> : null}
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
                  data-fresh={e.fresh && e.state === 'live' ? '' : undefined}
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

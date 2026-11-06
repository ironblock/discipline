import { useState, type FormEvent } from 'react';

import type { Ack, Command } from '../drive/transport.ts';
import type { Folded, MemoryEntry } from '../session/fold.ts';
import { authorityOf, opOf, refusalOf } from './sets.ts';
import { useHotEntries, useTarget } from './surface.tsx';
import './panel.css';
import './memory.css';

export interface MemoryProps {
  readonly entries: readonly Folded<MemoryEntry>[];
  /** Log position through which the person has acknowledged what landed. */
  readonly seenThrough?: number;
  /** Acknowledge everything that has landed so far. */
  readonly onSeen?: () => void;
  /** Where the operator's edits and flags go (#150): absent where the drive takes none (the canned script, a replay). */
  readonly act?: (command: Command) => Promise<Ack>;
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
export function Memory({ entries, seenThrough = -1, onSeen, act }: MemoryProps) {
  const target = useTarget();
  const hot = useHotEntries();
  const live = entries.filter((e) => e.state === 'live').length;
  const fresh = entries.filter((e) => isUnseen(e, seenThrough)).length;
  const categories = [...new Set(entries.map((e) => e.category))];
  return (
    <section className="ex-panel ex-memory" aria-label="working memory">
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
        <div className="ex-memory__category" key={category ?? ''}>
          {/* The predecessor kept one flat list: an entry with no category is listed without a heading. */}
          {category !== undefined ? <h3>{category}</h3> : null}
          <ul>
            {entries
              .filter((e) => e.category === category)
              .map((e) => (
                <li
                  key={e.id}
                  id={`memory/${e.id}`}
                  data-target={target === `memory/${e.id}` ? '' : undefined}
                  data-hot={hot.has(e.id) ? '' : undefined}
                  className="ex-memory__entry"
                  data-state={e.state}
                  data-fresh={isUnseen(e, seenThrough) ? '' : undefined}
                  data-from={e.from.join(' ')}
                  data-needs={e.needs.join(' ')}
                  data-lane={e.lane}
                  title={`${e.state}${e.authority ? ` · ${e.authority}` : ''} · last changed by ${e.by}`}
                >
                  <span className="ex-memory__id">#{e.id}</span>
                  <span className="ex-memory__text">
                    {e.text}
                    {e.op !== undefined ? <span className="ex-memory__op">{opOf(e.op).label}</span> : null}
                    {/* Who vouches for it (the app audit, finding 7): observed, the model's own words, yours. */}
                    {e.authority !== undefined ? <span className="ex-memory__authority">{authorityOf(e.authority).label}</span> : null}
                    {/* Written by the trunk's own lane, not a fork (#627): self-capture; or the operator's own keep. */}
                    {e.lane !== undefined && e.authority !== 'operator' ? (
                      <span className="ex-memory__lane" title={e.lane === 'operator' ? 'kept by you' : "written by the trunk's own lane, not by a fork"}>
                        {e.lane}
                      </span>
                    ) : null}
                  </span>
                  {/* The operator's flag (#150), kept until a meditation addresses it; and the model's writes refused over their entry. */}
                  {e.flag !== undefined ? <span className="ex-memory__flag">flag · {e.flag}</span> : null}
                  {/* The notes merged into it as similar (#872): each one's id, how similar, and what it said. */}
                  {e.merges && e.merges.length > 0 ? (
                    <ul className="ex-memory__merges" aria-label={`merged into #${e.id}`}>
                      {e.merges.map((m) => (
                        <li key={m.id} className="ex-memory__merge" title={`merged as similar by patch #${m.by}`}>
                          {`merged #${m.id} · cosine ${m.cosine} · ${m.text}`}
                        </li>
                      ))}
                    </ul>
                  ) : null}
                  {e.refusedWrites && e.refusedWrites.length > 0 ? (
                    <span className="ex-memory__refused" title="the model tried to change your entry, and was refused">
                      refused: {e.refusedWrites.map((w) => `${w.op} by ${w.by}`).join(', ')}
                    </span>
                  ) : null}
                  {/* A correction of it waiting beside its turn (#1050): the marker links to the chip. */}
                  {e.waiting && e.waiting.length > 0 ? (
                    <a className="ex-memory__waiting" href={`#correction-${e.waiting[0]!}`} title="the model's change to your entry waits on you beside its turn">
                      a correction is waiting
                    </a>
                  ) : null}
                  {act && e.state === 'live' ? <EntryActions id={e.id} text={e.text} act={act} yours={e.lane === 'operator'} /> : null}
                </li>
              ))}
          </ul>
        </div>
      ))}
    </section>
  );
}

/**
 * The operator's hand on one live entry (#150): rewrite it -- it supersedes, and the model may not overwrite it -- or
 * flag it with a note a meditation will address. Each answer the drive refuses is said in place.
 */
function EntryActions({ id, text, act, yours }: { readonly id: string; readonly text: string; readonly act: (command: Command) => Promise<Ack>; readonly yours: boolean }) {
  const [mode, setMode] = useState<'none' | 'edit' | 'flag'>('none');
  const [draft, setDraft] = useState(text);
  const [note, setNote] = useState('');
  const [refused, setRefused] = useState<string>();
  const send = async (command: Command) => {
    const ack = await act(command);
    if (ack.ok) {
      setMode('none');
      setRefused(undefined);
    } else setRefused(refusalOf(ack.refused).label);
  };
  // A form, so Return submits whichever mode is open; each control is named with its entry, so one query finds one.
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (mode === 'edit') void send({ kind: 'edit-entry', id, content: draft.trim() });
    else if (mode === 'flag') void send({ kind: 'flag-entry', id, note: note.trim() });
  };
  return (
    <form className="ex-memory__actions" onSubmit={submit}>
      {mode === 'edit' ? (
        <>
          <input
            type="text"
            aria-label={`Edit ${id}`}
            value={draft}
            autoFocus
            onFocus={(e) => e.currentTarget.select()}
            onChange={(e) => setDraft(e.target.value)}
          />
          <button type="submit" data-action="save" aria-label={`Save edit ${id}`}>
            save
          </button>
          <button type="button" aria-label={`Cancel edit ${id}`} onClick={() => setMode('none')}>
            cancel
          </button>
        </>
      ) : mode === 'flag' ? (
        <>
          <input type="text" aria-label={`Flag ${id}`} value={note} placeholder="what is wrong with it?" autoFocus onChange={(e) => setNote(e.target.value)} />
          <button type="submit" data-action="send-flag" aria-label={`Send flag ${id}`}>
            flag
          </button>
          <button type="button" aria-label={`Cancel flag ${id}`} onClick={() => setMode('none')}>
            cancel
          </button>
        </>
      ) : (
        <>
          <button type="button" data-action="edit" aria-label={`Edit ${id}`} title="rewrite this entry: yours supersedes it, and the model may not overwrite it" onClick={() => (setDraft(text), setMode('edit'))}>
            edit
          </button>
          <button type="button" data-action="flag" aria-label={`Flag ${id}`} title="flag this entry with a note: the next meditation addresses it" onClick={() => setMode('flag')}>
            flag
          </button>
          {/* The operator's retire of their own entry (#1050): it stops mattering, and is kept. */}
          {yours ? (
            <button type="button" data-action="retire" aria-label={`Retire ${id}`} title="retire this entry of yours: it stops mattering, and is kept" onClick={() => void send({ kind: 'retire-entry', id })}>
              retire
            </button>
          ) : null}
        </>
      )}
      {refused ? <span className="ex-memory__refusal">{refused}</span> : null}
    </form>
  );
}

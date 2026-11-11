import { Fragment, useRef, useState, type FormEvent } from 'react';

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
                  title={`${e.state}${e.authority ? ` · ${e.authority}` : ''} · last changed at log line ${e.by}`}
                >
                  <span className="ex-memory__id">#{e.id}</span>
                  <span className="ex-memory__text">
                    {e.text}
                    {e.op !== undefined ? <span className="ex-memory__op">{opOf(e.op).label}</span> : null}
                    {/* Who vouches for it (the app audit, finding 7): observed, the model's own words, yours. */}
                    {e.authority !== undefined ? <span className="ex-memory__authority">{authorityOf(e.authority).label}</span> : null}
                    {/* Where it came from, for every entry (the app audit's page walk, F8): the trunk's lane, or a fork's. An entry
                        whose authority is the operator's already says "kept by you", so its source word would say it twice. */}
                    {e.source !== undefined && e.authority !== 'operator' ? (
                      <span className="ex-memory__source" title={sourceTitle(e.source)}>
                        {e.source}
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
                  {/* The corrections of it waiting beside their turns (#1050), counted, each linked to its chip (F9). */}
                  {e.waiting && e.waiting.length > 0 ? (
                    <span className="ex-memory__waiting" title="the model's changes to your entry wait on you beside their turns">
                      {e.waiting.length === 1 ? 'a correction is waiting' : `${e.waiting.length} corrections waiting`}
                      {e.waiting.map((seq, i) => (
                        <a key={seq} href={`#correction-${seq}`} aria-label={`correction ${i + 1} of ${e.waiting!.length}`}>
                          {e.waiting!.length === 1 ? '' : ` ${i + 1}`}
                        </a>
                      ))}
                    </span>
                  ) : null}
                  {act && e.state === 'live' ? <EntryActions id={e.id} text={e.text} act={act} yours={e.lane === 'operator' || e.authority === 'operator'} /> : null}
                </li>
              ))}
          </ul>
        </div>
      ))}
    </section>
  );
}

/** What an entry's source word means, on hover. */
function sourceTitle(source: string): string {
  switch (source) {
    case 'operator':
      return 'kept by you';
    case 'self-capture':
      return "the model's own note, written as it went";
    case 'mechanical':
      return 'written by the harness, from the commands that ran';
    default:
      return `written by ${/^[aeiou]/.test(source) ? 'an' : 'a'} ${source} fork`;
  }
}

/** How long "retire it?" shows before a press answers it. */
const RETIRE_CONFIRM_AFTER_MS = 500;

/**
 * The operator's hand on one live entry (#150): rewrite it -- it supersedes, and the model may not overwrite it -- or
 * flag it with a note a meditation will address. Each answer the drive refuses is said in place.
 */
function EntryActions({ id, text, act, yours }: { readonly id: string; readonly text: string; readonly act: (command: Command) => Promise<Ack>; readonly yours: boolean }) {
  const [mode, setMode] = useState<'none' | 'edit' | 'flag'>('none');
  const [retiring, setRetiring] = useState(false);
  const retireArmedAt = useRef(0);
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
      {/* Each mode's controls keyed apart: unkeyed, React reused the `flag` button as the flag mode's submit button at
          the same position, and the click that opened the mode submitted an empty flag. */}
      {mode === 'edit' ? (
        <Fragment key="edit">
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
        </Fragment>
      ) : mode === 'flag' ? (
        <Fragment key="flag">
          <input type="text" aria-label={`Flag ${id}`} value={note} placeholder="what is wrong with it?" autoFocus onChange={(e) => setNote(e.target.value)} />
          <button type="submit" data-action="send-flag" aria-label={`Send flag ${id}`}>
            flag
          </button>
          <button type="button" aria-label={`Cancel flag ${id}`} onClick={() => setMode('none')}>
            cancel
          </button>
        </Fragment>
      ) : (
        <Fragment key="none">
          <button type="button" data-action="edit" aria-label={`Edit ${id}`} title="rewrite this entry: yours supersedes it, and the model may not overwrite it" onClick={() => (setDraft(text), setMode('edit'))}>
            edit
          </button>
          <button type="button" data-action="flag" aria-label={`Flag ${id}`} title="flag this entry with a note: the next meditation addresses it" onClick={() => setMode('flag')}>
            flag
          </button>
          {/* The operator's retire of their own entry (#1050): it stops mattering, and is kept. The first press asks, a
              second one -- after the question could be read -- retires (the app audit's page walk, F11), as end does. */}
          {yours ? (
            <button
              type="button"
              data-action="retire"
              data-armed={retiring || undefined}
              aria-label={`Retire ${id}`}
              title="retire this entry of yours: it stops mattering, and is kept"
              onClick={() => {
                if (!retiring) {
                  retireArmedAt.current = performance.now();
                  return setRetiring(true);
                }
                if (performance.now() - retireArmedAt.current < RETIRE_CONFIRM_AFTER_MS) return;
                setRetiring(false);
                void send({ kind: 'retire-entry', id });
              }}
              onBlur={() => setRetiring(false)}
            >
              {retiring ? 'retire it?' : 'retire'}
            </button>
          ) : null}
        </Fragment>
      )}
      {refused ? <span className="ex-memory__refusal">{refused}</span> : null}
    </form>
  );
}

import { createContext, useContext, useEffect, useRef, useState } from 'react';
import type { FormEvent, KeyboardEvent } from 'react';

import { FORK_ASKS } from '../../../diet/dogma/fork-asks.ts';
import type { ForkAsk } from '../../../diet/dogma/fork-asks.ts';
import type { Ack, Command } from '../drive/transport.ts';
import type { LoggedText, Shadowed } from '../session/fold.ts';
import { refusalOf } from './sets.ts';
import './gestures.css';

/**
 * The operator's gestures (#1021), where the drive takes them: the served drive's dispatch. Absent -- the canned
 * script, a replay -- no block offers a fork or a keep.
 */
export const GestureContext = createContext<((command: Command) => Promise<Ack>) | undefined>(undefined);

export function useGestures(): ((command: Command) => Promise<Ack>) | undefined {
  return useContext(GestureContext);
}

/** The pinned asks the picker shows first (Dispatch's ruling, 2026-11-03); the rest are under "more". */
const FIRST = ['GOTCHAS', 'DECISIONS', 'CONSTRAINTS', 'OPEN_NEXT', 'RATIONALE'];

/** The pinned asks, split: those first, in that order, then the rest in the dogma's. */
export function pickerAsks(asks: readonly ForkAsk[] = FORK_ASKS): { readonly first: readonly ForkAsk[]; readonly more: readonly ForkAsk[] } {
  const first = FIRST.flatMap((name) => asks.filter((ask) => ask.name === name));
  return { first, more: asks.filter((ask) => !FIRST.includes(ask.name)) };
}

/** What a refused gesture says: its label, and the entry a refusal names. */
function refusedOf(ack: Extract<Ack, { ok: false }>): string {
  const label = refusalOf(ack.refused).label;
  return ack.entry !== undefined ? `${label} (${ack.entry})` : label;
}

/** A dialog that opens modal while `open`, and says when it closes -- Escape, the form's own close, or a post. */
function useModal(open: boolean, onClose: () => void) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  }, [open]);
  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    dialog.addEventListener('close', onClose);
    return () => dialog.removeEventListener('close', onClose);
  }, [onClose]);
  return ref;
}

/**
 * Fork off the trunk here (#1021): a pinned ask -- each showing the text it sends -- or the operator's own words.
 * The fork writes nothing to working memory; its answer comes back beside the trunk, to keep from.
 */
export function ForkAction({ at, label }: { readonly at: LoggedText; readonly label: string }) {
  const act = useGestures();
  const [open, setOpen] = useState(false);
  const [text, setText] = useState('');
  const [refused, setRefused] = useState<string>();
  const close = () => setOpen(false);
  const dialog = useModal(open, close);
  if (!act) return null;
  const { first, more } = pickerAsks();
  const send = async (command: Command) => {
    const ack = await act(command);
    if (ack.ok) {
      setText('');
      setRefused(undefined);
      setOpen(false);
    } else setRefused(refusedOf(ack));
  };
  const pinned = (ask: ForkAsk) => (
    <li key={ask.name}>
      <button type="button" className="ex-gesture__ask" aria-label={`Fork at ${label} asking ${ask.name}`} onClick={() => void send({ kind: 'fork', at: at.seq, ask: ask.name })}>
        <span className="ex-gesture__ask-name">{ask.name}</span>
        <span className="ex-gesture__ask-text">{ask.text.trim()}</span>
      </button>
    </li>
  );
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (text.trim() !== '') void send({ kind: 'fork', at: at.seq, text: text.trim() });
  };
  // Return asks; Shift-Return is a new line.
  const keys = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) submit(e);
  };
  return (
    <>
      <button type="button" className="ex-action" aria-label={`Fork at ${label}`} title="fork here: a pinned ask, or your own words -- it writes nothing to working memory" onClick={() => setOpen(true)}>
        fork
      </button>
      <dialog ref={dialog} className="ex-gesture" aria-label={`Fork at ${label}`}>
        {open ? (
          <form onSubmit={submit}>
            <h2 className="ex-gesture__title">fork at {label}</h2>
            <ul className="ex-gesture__asks">{first.map(pinned)}</ul>
            {more.length > 0 ? (
              <details className="ex-gesture__more">
                <summary>more</summary>
                <ul className="ex-gesture__asks">{more.map(pinned)}</ul>
              </details>
            ) : null}
            <label className="ex-gesture__own">
              your own words
              <textarea aria-label={`Your own question at ${label}`} value={text} autoFocus onChange={(e) => setText(e.target.value)} onKeyDown={keys} />
            </label>
            <div className="ex-gesture__row">
              <button type="submit" aria-label={`Ask at ${label}`} disabled={text.trim() === ''}>
                ask
              </button>
              <button type="button" onClick={close}>
                cancel
              </button>
              {refused ? <span className="ex-gesture__refusal" role="alert">{refused}</span> : null}
            </div>
          </form>
        ) : null}
      </dialog>
    </>
  );
}

/**
 * Keep an exact span of this line (#1021): its logged text, as the drive matches a keep against it, in a plain view
 * to select from -- never the rendered one, whose markdown and clipping are not what was logged. The span posts with
 * an optional tag, and the view closes.
 */
export function KeepAction({ logged, label }: { readonly logged: LoggedText; readonly label: string }) {
  const act = useGestures();
  const [open, setOpen] = useState(false);
  const [tag, setTag] = useState('');
  const [refused, setRefused] = useState<string>();
  const raw = useRef<HTMLPreElement>(null);
  const close = () => setOpen(false);
  const dialog = useModal(open, close);
  useEffect(() => {
    if (open) raw.current?.focus();
  }, [open]);
  if (!act) return null;
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const selection = window.getSelection();
    const inside = selection && raw.current && selection.rangeCount > 0 && raw.current.contains(selection.getRangeAt(0).commonAncestorContainer);
    const span = inside ? selection.toString() : '';
    if (span.trim() === '') return setRefused('select the span to keep first');
    const ack = await act({ kind: 'keep', from: logged.seq, span, ...(tag.trim() !== '' ? { tag: tag.trim() } : {}) });
    if (ack.ok) {
      setTag('');
      setRefused(undefined);
      setOpen(false);
    } else setRefused(refusedOf(ack));
  };
  return (
    <>
      <button type="button" className="ex-action" aria-label={`Keep from ${label}`} title="keep a span of this, exactly as logged, as your entry in working memory" onClick={() => setOpen(true)}>
        keep
      </button>
      <dialog ref={dialog} className="ex-gesture" aria-label={`Keep from ${label}`}>
        {open ? (
          <form onSubmit={(e) => void submit(e)}>
            <h2 className="ex-gesture__title">keep from {label}</h2>
            <p className="ex-gesture__hint">select the span to keep, exactly as logged</p>
            <pre ref={raw} className="ex-gesture__raw" tabIndex={0} aria-label={`What ${label} logged`}>
              {logged.text}
            </pre>
            <div className="ex-gesture__row">
              <input type="text" aria-label={`Tag for the keep from ${label}`} placeholder="tag (optional)" value={tag} onChange={(e) => setTag(e.target.value)} />
              <button type="submit" aria-label={`Keep the selection from ${label}`}>
                keep
              </button>
              <button type="button" onClick={close}>
                cancel
              </button>
              {refused ? <span className="ex-gesture__refusal" role="alert">{refused}</span> : null}
            </div>
          </form>
        ) : null}
      </dialog>
    </>
  );
}

/**
 * Where the automation would have fired here (#1021): each fork a shadow warrant would have cut at this step, never
 * sent -- quiet, so the operator can see it without its being asked for.
 */
export function ShadowMarks({ shadowed }: { readonly shadowed: readonly Shadowed[] | undefined }) {
  if (!shadowed || shadowed.length === 0) return null;
  return (
    <ul className="ex-shadow" aria-label="where a shadow warrant would have forked">
      {shadowed.map((shadow, at) => (
        <li key={at} className="ex-shadow__mark" title={`${shadow.trigger}: never sent`}>
          would fork · {shadow.why} → {shadow.ask}
        </li>
      ))}
    </ul>
  );
}

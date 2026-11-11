import { createContext, useContext, useEffect, useRef, useState } from 'react';
import type { FormEvent, KeyboardEvent } from 'react';

import type { ForkAsk } from '../../../diet/dogma/fork-asks.ts';
import type { Ack, Command } from '../drive/transport.ts';
import type { Correction, LoggedText, Shadowed } from '../session/fold.ts';
import { pickerAsks } from './forkAsks.ts';
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

/**
 * What serve will take a gesture at now (the app audit, finding 10): a fork, and a suggestion's accept that fires one,
 * only while the session awaits; a fork or a wrap only at a line still on the trunk, which a meditation's refill takes
 * off. Outside a session view, everything is taken.
 */
export interface GestureMoment {
  readonly awaiting: boolean;
  readonly onTrunk: (seq: number) => boolean;
  /** Whether the result at a call's line can still be wrapped: a result wraps once (the app audit's page walk, F17). */
  readonly wrappable?: (call: number) => boolean;
  /** The result at a call's line, named for the operator: which call, and what it ran. */
  readonly resultOf?: (call: number) => string | undefined;
  /** Each live entry's text, by id: what a correction would change (F9). */
  readonly entryText?: (id: string) => string | undefined;
}

export const GestureMomentContext = createContext<GestureMoment>({ awaiting: true, onTrunk: () => true });

/** Why a fork waits, on its disabled control. */
const FORK_WAITS = 'fork when the turn or fork settles';

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
    // `close` arrives a task after the dialog closed: one that arrives after it opened again is stale, and is not heard.
    const closed = () => {
      if (!dialog.open) onClose();
    };
    dialog.addEventListener('close', closed);
    return () => dialog.removeEventListener('close', closed);
  }, [onClose]);
  return ref;
}

/**
 * Fork off the trunk here (#1021): a pinned ask -- each showing the text it sends -- or the operator's own words.
 * The fork writes nothing to working memory; its answer comes back beside the trunk, to keep from.
 */
export function ForkAction({ at, label }: { readonly at: LoggedText; readonly label: string }) {
  const act = useGestures();
  const moment = useContext(GestureMomentContext);
  const [open, setOpen] = useState(false);
  const [text, setText] = useState('');
  const [refused, setRefused] = useState<string>();
  const close = () => setOpen(false);
  const dialog = useModal(open, close);
  // The pinned ask opened to its whole text, to read before it is sent (the app audit's page walk, F4).
  const [reading, setReading] = useState<string>();
  // `showModal()` focuses the first control, a pinned ask, where an Enter would send it; "your own words" takes it.
  const own = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    if (open) own.current?.focus();
  }, [open]);
  if (!act || !moment.onTrunk(at.seq)) return null;
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
      <button
        type="button"
        className="ex-gesture__ask"
        aria-label={`Fork at ${label} asking ${ask.name}`}
        aria-expanded={reading === ask.name}
        onClick={() => setReading(reading === ask.name ? undefined : ask.name)}
      >
        <span className="ex-gesture__ask-name">{ask.name}</span>
        <span className="ex-gesture__ask-text" data-whole={reading === ask.name ? '' : undefined}>
          {ask.text.trim()}
        </span>
      </button>
      {reading === ask.name ? (
        <button type="button" className="ex-gesture__send" aria-label={`Ask ${ask.name} at ${label}`} onClick={() => void send({ kind: 'fork', at: at.seq, ask: ask.name })}>
          ask this
        </button>
      ) : null}
    </li>
  );
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (text.trim() !== '') void send({ kind: 'fork', at: at.seq, text: text.trim() });
  };
  // Return asks; Shift-Return is a new line.
  const keys = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    // Not while an input method is composing: that Enter picks a candidate (the app audit, 22).
    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) submit(e);
  };
  return (
    <>
      <button
        type="button"
        className="ex-action"
        aria-label={`Fork at ${label}`}
        title={moment.awaiting ? 'fork here: a pinned ask, or your own words -- it writes nothing to working memory' : FORK_WAITS}
        disabled={!moment.awaiting}
        onClick={() => {
          setRefused(undefined);
          setReading(undefined);
          setOpen(true);
        }}
      >
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
              <textarea ref={own} aria-label={`Your own question at ${label}`} value={text} onChange={(e) => setText(e.target.value)} onKeyDown={keys} />
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
  // The span last selected in either text (the app audit, finding 2): a click into the tag field moves the document's
  // selection away, and the span stays what the person selected.
  const [captured, setCaptured] = useState('');
  const raw = useRef<HTMLPreElement>(null);
  const thought = useRef<HTMLPreElement>(null);
  const close = () => setOpen(false);
  const dialog = useModal(open, close);
  useEffect(() => {
    if (open) raw.current?.focus();
  }, [open]);
  useEffect(() => {
    if (!open) return;
    setCaptured('');
    const selected = () => {
      const selection = window.getSelection();
      if (!selection || selection.rangeCount === 0 || selection.toString() === '') return;
      const node = selection.getRangeAt(0).commonAncestorContainer;
      if (raw.current?.contains(node) || thought.current?.contains(node)) setCaptured(selection.toString());
    };
    document.addEventListener('selectionchange', selected);
    return () => document.removeEventListener('selectionchange', selected);
  }, [open]);
  if (!act) return null;
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const selection = window.getSelection();
    const node = selection && selection.rangeCount > 0 ? selection.getRangeAt(0).commonAncestorContainer : undefined;
    const inside = node !== undefined && (raw.current?.contains(node) || thought.current?.contains(node));
    const span = selection && inside && selection.toString() !== '' ? selection.toString() : captured;
    if (span.trim() === '') return setRefused('select the span to keep first');
    const ack = await act({ kind: 'keep', from: logged.seq, span, ...(tag.trim() !== '' ? { tag: tag.trim() } : {}) });
    if (ack.ok) {
      setTag('');
      setRefused(undefined);
      setOpen(false);
    } else setRefused(refusedOf(ack));
  };
  const reasoning = logged.reasoning ?? '';
  return (
    <>
      <button
        type="button"
        className="ex-action"
        aria-label={`Keep from ${label}`}
        title="keep a span of this, exactly as logged, as your entry in working memory"
        onClick={() => {
          setRefused(undefined);
          setOpen(true);
        }}
      >
        keep
      </button>
      <dialog ref={dialog} className="ex-gesture" aria-label={`Keep from ${label}`}>
        {open ? (
          <form onSubmit={(e) => void submit(e)}>
            <h2 className="ex-gesture__title">keep from {label}</h2>
            <p className="ex-gesture__hint">select the span to keep, exactly as logged</p>
            {logged.text !== '' ? (
              <pre ref={raw} className="ex-gesture__raw" tabIndex={0} aria-label={`What ${label} logged`}>
                {logged.text}
              </pre>
            ) : null}
            {/* What it reasoned (#1037): a fork's right answer may be only there, and serve keeps a span of it too. */}
            {reasoning !== '' ? (
              <pre ref={thought} className="ex-gesture__raw ex-gesture__raw--reasoning" tabIndex={0} aria-label={`What ${label} reasoned`}>
                {reasoning}
              </pre>
            ) : null}
            {captured !== '' ? <p className="ex-gesture__captured">keeping: {captured}</p> : null}
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

/** The wrap dialog's title: the result it replaces, with a span of it or of the answer it is wrapped from, once. */
function wrapTitle(result: string | undefined, label: string, itself: boolean): string {
  const what = result ?? `${label}'s result`;
  return itself ? `wrap ${what} with a span of it: once, from the next request` : `wrap ${what} with a span of ${label}'s answer: once, from the next request`;
}

/**
 * Wrap a tool result (#1030): a span of `from` -- the result as the model was shown it, or the answer to the operator's
 * fork cut at it -- takes the result's place on the trunk from the next request, and is kept as the operator's entry.
 * The request in flight stands as sent; everything after the result is prefilled again.
 */
export function WrapAction({ call, from, label }: { readonly call: number; readonly from: LoggedText; readonly label: string }) {
  const act = useGestures();
  const moment = useContext(GestureMomentContext);
  const [open, setOpen] = useState(false);
  const [refused, setRefused] = useState<string>();
  // The span last selected in the text, as keep holds it (the app audit, finding 2): the click on `wrap` can move the
  // document's selection away before the submit reads it, and the span stays what the person selected.
  const [captured, setCaptured] = useState('');
  const raw = useRef<HTMLPreElement>(null);
  const close = () => setOpen(false);
  const dialog = useModal(open, close);
  useEffect(() => {
    if (open) raw.current?.focus();
  }, [open]);
  useEffect(() => {
    if (!open) return;
    setCaptured('');
    const selected = () => {
      const selection = window.getSelection();
      if (!selection || selection.rangeCount === 0 || selection.toString() === '') return;
      if (raw.current?.contains(selection.getRangeAt(0).commonAncestorContainer)) setCaptured(selection.toString());
    };
    document.addEventListener('selectionchange', selected);
    return () => document.removeEventListener('selectionchange', selected);
  }, [open]);
  if (!act || !moment.onTrunk(call) || moment.wrappable?.(call) === false) return null;
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const selection = window.getSelection();
    const inside = selection && raw.current && selection.rangeCount > 0 && raw.current.contains(selection.getRangeAt(0).commonAncestorContainer);
    const span = inside && selection.toString() !== '' ? selection.toString() : captured;
    if (span.trim() === '') return setRefused('select the span to keep in its place first');
    const ack = await act({ kind: 'wrap', call, from: from.seq, span });
    if (ack.ok) {
      setRefused(undefined);
      setOpen(false);
    } else setRefused(refusedOf(ack));
  };
  return (
    <>
      <button
        type="button"
        className="ex-action"
        aria-label={`Wrap from ${label}`}
        title="replace the result on the trunk by a span you keep, from the next request on"
        onClick={() => {
          setRefused(undefined);
          setOpen(true);
        }}
      >
        wrap
      </button>
      <dialog ref={dialog} className="ex-gesture" aria-label={`Wrap from ${label}`}>
        {open ? (
          <form onSubmit={(e) => void submit(e)}>
            {/* Which result it replaces, from what, and that it is once (the app audit's page walk, F17). */}
            <h2 className="ex-gesture__title">{wrapTitle(moment.resultOf?.(call), label, from.seq === call)}</h2>
            <p className="ex-gesture__hint">select the span the trunk keeps in the result’s place, from the next request on; everything after it is prefilled again</p>
            <pre ref={raw} className="ex-gesture__raw" tabIndex={0} aria-label={`What ${label} logged, to wrap`}>
              {from.text}
            </pre>
            <div className="ex-gesture__row">
              <button type="submit" aria-label={`Wrap with the selection from ${label}`}>
                wrap
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
 * sent -- shown in a replay and the read-only views, to review afterwards against where the operator forked, with how
 * the operator answered each one offered. During a live drive (where the gestures are taken) only a session whose start
 * line says `chips` shows them, each as a suggestion to accept or reject (#1031): seen there, a shadow is no longer
 * independent of the operator's own forks, which is why the start line says which mode ran. Hidden, the log keeps them.
 */
export function ShadowMarks({ shadowed }: { readonly shadowed: readonly Shadowed[] | undefined }) {
  const act = useGestures();
  if (!shadowed || shadowed.length === 0) return null;
  if (act) {
    const offered = shadowed.filter((shadow) => shadow.offered);
    if (offered.length === 0) return null;
    return (
      <ul className="ex-shadow ex-shadow--offered" aria-label="suggestions from the shadow warrants">
        {offered.map((shadow) => (
          <SuggestionChip key={shadow.seq} shadow={shadow} act={act} />
        ))}
      </ul>
    );
  }
  return (
    <ul className="ex-shadow" aria-label="where a shadow warrant would have forked">
      {shadowed.map((shadow) => (
        <li key={shadow.seq} className="ex-shadow__mark" title={`${shadow.trigger}: never sent`}>
          would fork · {shadow.why} → {shadow.ask}
          {shadow.answer ? ` · ${shadow.answer}` : ''}
        </li>
      ))}
    </ul>
  );
}

/**
 * One shadow offered as a suggestion (#1031): its warrant, its trigger and the question its fork would ask. Accept
 * fires that fork as the operator's; reject declines it. Either answer is logged, and so is one never given.
 */
function SuggestionChip({ shadow, act }: { readonly shadow: Shadowed; readonly act: (command: Command) => Promise<Ack> }) {
  const moment = useContext(GestureMomentContext);
  const [busy, setBusy] = useState(false);
  const [refused, setRefused] = useState<string>();
  if (shadow.answer) {
    return (
      <li className="ex-shadow__mark" title={shadow.trigger}>
        {shadow.why} → {shadow.ask} · {shadow.answer}
      </li>
    );
  }
  const question = shadow.question ?? shadow.ask;
  const answer = async (word: 'accept' | 'reject') => {
    setBusy(true);
    const ack = await act({ kind: 'suggestion', shadow: shadow.seq, answer: word });
    setBusy(false);
    setRefused(ack.ok ? undefined : refusedOf(ack));
  };
  return (
    <li className="ex-shadow__chip">
      <span className="ex-shadow__said">this automatic fork would have fired, using this question:</span>
      <q className="ex-shadow__question">{question}</q>
      <span className="ex-shadow__why">
        {shadow.why} · {shadow.trigger}
      </span>
      {/* Accepted, it fires a fork: serve takes that only while the session awaits. */}
      <button type="button" disabled={busy || !moment.awaiting} title={moment.awaiting ? undefined : FORK_WAITS} aria-label={`Accept: ${question}`} onClick={() => void answer('accept')}>
        Accept
      </button>
      <button type="button" disabled={busy} aria-label={`Reject: ${question}`} onClick={() => void answer('reject')}>
        Reject
      </button>
      {refused ? <span role="alert">{refused}</span> : null}
    </li>
  );
}

/** What a refused change would have done to the operator's entry (#1050), as its chip says it. */
function changeOf(correction: Correction): string {
  const verb = correction.op === 'supersede' ? 'replace' : correction.op;
  const head = `The model would ${verb} your entry`;
  if (correction.op === 'supersede' && correction.text !== undefined) return `${head} with:`;
  return correction.why !== undefined ? `${head}: ${correction.why}` : head;
}

/**
 * The model's changes to the operator's entries that the guard refused at this step (#1050), beside the turn that made
 * them: while driving, each a chip to accept -- the change applies as the operator's ruling -- or reject. Either answer
 * is logged, and so is one never given. Read-only views show what each came to.
 */
export function CorrectionChips({ corrections }: { readonly corrections: readonly Correction[] | undefined }) {
  const act = useGestures();
  if (!corrections || corrections.length === 0) return null;
  return (
    <ul className="ex-shadow ex-shadow--offered" aria-label="the model's refused changes to your entries">
      {corrections.map((correction) => (
        <CorrectionChip key={correction.seq} correction={correction} act={act} />
      ))}
    </ul>
  );
}

function CorrectionChip({ correction, act }: { readonly correction: Correction; readonly act: ((command: Command) => Promise<Ack>) | undefined }) {
  const moment = useContext(GestureMomentContext);
  const [busy, setBusy] = useState(false);
  const [refused, setRefused] = useState<string>();
  const said = changeOf(correction);
  // Which entry, and what it says now, beside what the model would make of it (the app audit's page walk, F9).
  const current = moment.entryText?.(correction.entry);
  const entry = <span className="ex-shadow__entry">{current !== undefined ? `#${correction.entry}: “${current}”` : `#${correction.entry}`}</span>;
  if (correction.answer || !act) {
    return (
      <li id={`correction-${correction.seq}`} className="ex-shadow__mark" title={`#${correction.entry}`}>
        {entry} {said}
        {correction.op === 'supersede' && correction.text !== undefined ? ` ${correction.text}` : ''}
        {correction.answer ? ` · ${correction.answer}` : ''}
      </li>
    );
  }
  const answer = async (word: 'accept' | 'reject') => {
    setBusy(true);
    const ack = await act({ kind: 'correction', refused: correction.seq, answer: word });
    setBusy(false);
    setRefused(ack.ok ? undefined : refusedOf(ack));
  };
  return (
    <li id={`correction-${correction.seq}`} className="ex-shadow__chip" title={`#${correction.entry}`}>
      {entry}
      <span className="ex-shadow__said">{said}</span>
      {correction.op === 'supersede' && correction.text !== undefined ? <q className="ex-shadow__question">{correction.text}</q> : null}
      <button type="button" disabled={busy} aria-label={`Accept: ${said} ${correction.entry}`} onClick={() => void answer('accept')}>
        Accept
      </button>
      <button type="button" disabled={busy} aria-label={`Reject: ${said} ${correction.entry}`} onClick={() => void answer('reject')}>
        Reject
      </button>
      {refused ? <span role="alert">{refused}</span> : null}
    </li>
  );
}

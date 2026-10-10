import { useEffect, useRef, useState } from 'react';
import type { ClipboardEvent, DragEvent, FormEvent, KeyboardEvent, MouseEvent } from 'react';

import type { SessionState } from '../session/fold.ts';
import type { Ack, Command, Link, Uploaded } from '../drive/transport.ts';
import { refusalOf } from './sets.ts';
import './panel.css';
import './composer.css';

export interface ComposerProps {
  readonly state: SessionState;
  /** The connection to the drive. While it is down the draft is kept and nothing can be sent. */
  readonly link?: Link;
  readonly phase: string;
  /** Phases the person may declare a transition to. None: the drive declares none, and a refill names no phase. */
  readonly phases: readonly string[];
  /** Where commands go. Absent: a composer that only shows the session's state. */
  readonly dispatch?: (command: Command) => Promise<Ack>;
  /** A line under the input, for a transport that has something to say. */
  readonly hint?: string | undefined;
  /** Where the operator's PNGs go ahead of the ask that names them. Absent: nothing can be attached. */
  readonly upload?: (bytes: Uint8Array) => Promise<Uploaded>;
}

/** A PNG the drive has taken, waiting for the ask that names it: its digest, and a picture of it for the chip. */
interface Attached {
  readonly sha256: string;
  readonly name: string;
  readonly url: string;
}

/** What the input says about the session, so a busy drive never looks like a stuck input. */
const STATE_LINE: Readonly<Record<SessionState, string>> = {
  connecting: 'connecting…',
  awaiting: 'your turn',
  turn: 'working · esc cancels',
  capture: 'interview running · send when it settles',
  ratify: 'ratifying before the refill',
  ended: 'the session has ended',
};

/** How long the question "end the session?" shows before a press answers it. */
const CONFIRM_AFTER_MS = 500;

export function Composer({ state, link = 'live', phase, phases, dispatch, hint, upload }: ComposerProps) {
  const [draft, setDraft] = useState('');
  // The operator's attachments (#372): uploaded as they are added, named by digest on the next ask, cleared once taken.
  const [attached, setAttached] = useState<readonly Attached[]>([]);
  // Uploads still out: an ask waits for them, so a screenshot never rides the ask after the one it was meant for.
  const [uploading, setUploading] = useState(0);
  const picker = useRef<HTMLInputElement>(null);
  const urls = useRef(new Set<string>());
  useEffect(() => () => urls.current.forEach((url) => URL.revokeObjectURL(url)), []);
  // The operator's mark on the next ask: the scope answer (#453). It rides on that ask only, and clears once taken.
  const [scoping, setScoping] = useState(false);
  const [refusal, setRefusal] = useState<string | undefined>();
  const next = phases[phases.indexOf(phase) + 1] ?? phases.find((p) => p !== phase) ?? phase;
  const [to, setTo] = useState(next);
  // Ending cannot be taken back: the first press asks, a second one sends (#289) -- a second the person chose after
  // seeing the question, not the other half of a double click or a held key.
  const [ending, setEnding] = useState(false);
  const armedAt = useRef(0);
  const idle = state === 'awaiting' && link === 'live';
  // Not idle, it cannot be taken; idle again, it asks again. (A disabled button keeps its focus and is never blurred.)
  useEffect(() => {
    if (!idle) setEnding(false);
  }, [idle]);
  const running = state === 'turn' || state === 'capture' || state === 'ratify';

  const answer = (ack: Ack) => {
    if (ack.ok) return setRefusal(undefined);
    const refused = refusalOf(ack.refused);
    setRefusal(refused.known ? refused.label : `not taken: ${refused.label}`);
  };
  const run = (command: Command) => dispatch && void dispatch(command).then(answer);

  // Whenever the operator can type: an upload does not depend on what the session is doing (`POST /files`).
  const attachable = dispatch !== undefined && upload !== undefined && link === 'live' && state !== 'ended' && state !== 'connecting';
  const attach = async (files: readonly File[]) => {
    if (!attachable) return;
    setUploading((n) => n + files.length);
    for (const file of files) {
      try {
        const taken = await upload(new Uint8Array(await file.arrayBuffer()));
        if (!taken.ok) {
          answer(taken);
          continue;
        }
        const url = URL.createObjectURL(file);
        urls.current.add(url);
        setAttached((now) => (now.some((a) => a.sha256 === taken.sha256) ? now : [...now, { sha256: taken.sha256, name: file.name || 'pasted image', url }]));
        setRefusal(undefined);
      } finally {
        setUploading((n) => n - 1);
      }
    }
  };
  /** Take the chips whose digests are DIGESTS off the composer, and let their pictures go. */
  const detach = (digests: readonly string[]) =>
    setAttached((now) => {
      for (const gone of now.filter((a) => digests.includes(a.sha256))) {
        URL.revokeObjectURL(gone.url);
        urls.current.delete(gone.url);
      }
      return now.filter((a) => !digests.includes(a.sha256));
    });
  const onPaste = (e: ClipboardEvent<HTMLTextAreaElement>) => {
    const images = [...e.clipboardData.files].filter((f) => f.type.startsWith('image/'));
    if (images.length === 0 || !attachable) return;
    e.preventDefault();
    void attach(images);
  };
  const onDrop = (e: DragEvent<HTMLFormElement>) => {
    if (!attachable || e.dataTransfer.files.length === 0) return;
    e.preventDefault();
    void attach([...e.dataTransfer.files]);
  };
  const end = (e: MouseEvent<HTMLButtonElement>) => {
    if (!ending) {
      armedAt.current = performance.now();
      return setEnding(true);
    }
    // The second click of a double click, or a press before the question could be read, is not an answer.
    if (e.detail > 1 || performance.now() - armedAt.current < CONFIRM_AFTER_MS) return;
    setEnding(false);
    run({ kind: 'end' });
  };

  const send = async (e?: FormEvent) => {
    e?.preventDefault();
    if (!dispatch || !idle || draft.trim() === '' || uploading > 0) return;
    const files = attached.map((a) => a.sha256);
    const ack = await dispatch({ kind: 'ask', text: draft.trim(), ...(scoping ? { scoping: true as const } : {}), ...(files.length > 0 ? { files } : {}) });
    answer(ack);
    if (ack.ok) {
      setDraft('');
      setScoping(false);
      // Only what this ask named: a chip added while it was out rides the next one.
      detach(files);
    }
  };

  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    // Not while an input method is composing: that Enter picks a candidate.
    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) void send(e);
    if (e.key === 'Escape' && running && dispatch) {
      e.preventDefault();
      run({ kind: 'cancel' });
    }
  };

  return (
    <form
      className="ex-panel ex-composer"
      onSubmit={send}
      onDragOver={(e) => attachable && e.dataTransfer.types.includes('Files') && e.preventDefault()}
      onDrop={onDrop}
      data-state={state}
      data-link={link}
    >
      {attached.length > 0 ? (
        <ul className="ex-composer__files" aria-label="attached to this ask">
          {attached.map((a) => (
            <li key={a.sha256} className="ex-composer__file" data-sha256={a.sha256}>
              <img src={a.url} alt={a.name} />
              <button type="button" aria-label={`remove ${a.name}`} title={`remove ${a.name}`} onClick={() => detach([a.sha256])}>
                ×
              </button>
            </li>
          ))}
        </ul>
      ) : null}
      <textarea
        className="ex-composer__input"
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={onKey}
        onPaste={onPaste}
        placeholder={idle ? (scoping ? 'The scope answer…' : 'Ask…') : ''}
        disabled={state === 'ended' || state === 'connecting'}
        rows={2}
        aria-label="your ask"
      />
      <div className="ex-composer__bar">
        <span className="ex-composer__state" role="status">
          {link === 'reconnecting'
            ? 'reconnecting to the drive · your draft is kept'
            : link === 'lost'
              ? 'the connection to the drive is lost · your draft is kept'
              : (refusal ?? STATE_LINE[state])}
        </span>
        <span className="ex-composer__spacer" />
        {/* As `diet` takes it: only while awaiting. Leaving the button, or the composer leaving idle, disarms it. */}
        <button
          type="button"
          className="ex-composer__end"
          data-armed={ending || undefined}
          disabled={!idle || !dispatch}
          onClick={end}
          // A held Enter repeats; a repeat is not a second answer.
          onKeyDown={(e) => e.repeat && e.preventDefault()}
          onBlur={() => setEnding(false)}
        >
          {ending ? 'end the session?' : 'end'}
        </button>
        <span className="ex-composer__phase">
          <span className="ex-composer__label">phase</span> {phase || (phases.length === 0 ? 'not declared' : 'not said')}
        </span>
        <span className="ex-composer__seam">
          {/* The moves the graph allows from here (#563); with none -- no graph, or the last phase -- a refill names no phase. */}
          {phases.length > 0 ? (
            <>
              <span className="ex-composer__label" aria-hidden="true">move to</span>
              <select aria-label="move to" value={to} onChange={(e) => setTo(e.target.value)} disabled={!idle}>
                {phases
                  .filter((p) => p !== phase)
                  .map((p) => (
                    <option key={p}>{p}</option>
                  ))}
              </select>
            </>
          ) : null}
          <button type="button" disabled={!idle || !dispatch} onClick={() => run(phases.length > 0 ? { kind: 'seam', to } : { kind: 'seam' })}>
            refill
          </button>
        </span>
        {upload ? (
          <>
            {/* The operator's screenshot (#372): picked, pasted or dropped, sent to the drive now, named by the ask. */}
            <input
              ref={picker}
              type="file"
              accept="image/png"
              multiple
              hidden
              aria-label="attach a PNG"
              onChange={(e) => {
                const files = [...(e.target.files ?? [])];
                e.target.value = '';
                void attach(files);
              }}
            />
            <button type="button" className="ex-composer__attach" disabled={!attachable} title="attach a PNG: or paste or drop one here" onClick={() => picker.current?.click()}>
              attach
            </button>
          </>
        ) : null}
        {running ? (
          <button type="button" className="ex-composer__cancel" disabled={!dispatch} onClick={() => run({ kind: 'cancel' })}>
            cancel
          </button>
        ) : (
          <>
            {/* The operator's mark (#453): this ask is the scope answer, whose settled turn warrants the interview fork. */}
            <button
              type="button"
              className="ex-composer__scope"
              aria-pressed={scoping}
              disabled={!idle || !dispatch}
              title="mark this ask the scope answer: its turn warrants the interview fork"
              onClick={() => setScoping(!scoping)}
            >
              scope answer
            </button>
            <button type="submit" className="ex-composer__send" disabled={!idle || !dispatch || draft.trim() === '' || uploading > 0} title={uploading > 0 ? 'waiting for the attachment to reach the drive' : undefined}>
              {scoping ? 'send as scope answer' : 'send'}
            </button>
          </>
        )}
      </div>
      {hint ? <p className="ex-composer__hint">{hint}</p> : null}
    </form>
  );
}

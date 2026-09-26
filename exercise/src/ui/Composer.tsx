import { useState } from 'react';
import type { FormEvent, KeyboardEvent } from 'react';

import type { SessionState } from '../session/fold.ts';
import type { Ack, Command, Link } from '../drive/transport.ts';
import { refusalOf } from './sets.ts';
import './composer.css';

export interface ComposerProps {
  readonly state: SessionState;
  /** The connection to the drive. While it is down the draft is kept and nothing can be sent. */
  readonly link?: Link;
  readonly phase: string;
  /** Phases the person may declare a transition to. */
  readonly phases: readonly string[];
  /** Where commands go. Absent: a composer that only shows the session's state. */
  readonly dispatch?: (command: Command) => Promise<Ack>;
  /** A line under the input, for a transport that has something to say. */
  readonly hint?: string | undefined;
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

export function Composer({ state, link = 'live', phase, phases, dispatch, hint }: ComposerProps) {
  const [draft, setDraft] = useState('');
  const [refusal, setRefusal] = useState<string | undefined>();
  const next = phases[phases.indexOf(phase) + 1] ?? phases.find((p) => p !== phase) ?? phase;
  const [to, setTo] = useState(next);
  const idle = state === 'awaiting' && link === 'live';
  const running = state === 'turn' || state === 'capture' || state === 'ratify';

  const answer = (ack: Ack) => {
    if (ack.ok) return setRefusal(undefined);
    const refused = refusalOf(ack.refused);
    setRefusal(refused.known ? refused.label : `not taken: ${refused.label}`);
  };
  const run = (command: Command) => dispatch && void dispatch(command).then(answer);

  const send = async (e?: FormEvent) => {
    e?.preventDefault();
    if (!dispatch || !idle || draft.trim() === '') return;
    const ack = await dispatch({ kind: 'ask', text: draft.trim() });
    answer(ack);
    if (ack.ok) setDraft('');
  };

  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) void send(e);
    if (e.key === 'Escape' && running && dispatch) {
      e.preventDefault();
      run({ kind: 'cancel' });
    }
  };

  return (
    <form className="ex-composer" onSubmit={send} data-state={state} data-link={link}>
      <textarea
        className="ex-composer__input"
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={onKey}
        placeholder={idle ? 'Ask…' : ''}
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
        <span className="ex-composer__phase">
          <span className="ex-composer__label">phase</span> {phase}
        </span>
        <label className="ex-composer__seam">
          <span className="ex-composer__label">move to</span>
          <select value={to} onChange={(e) => setTo(e.target.value)} disabled={!idle}>
            {phases
              .filter((p) => p !== phase)
              .map((p) => (
                <option key={p}>{p}</option>
              ))}
          </select>
          <button type="button" disabled={!idle || !dispatch} onClick={() => run({ kind: 'seam', to })}>
            refill
          </button>
        </label>
        {running ? (
          <button type="button" className="ex-composer__cancel" disabled={!dispatch} onClick={() => run({ kind: 'cancel' })}>
            cancel
          </button>
        ) : (
          <button type="submit" className="ex-composer__send" disabled={!idle || !dispatch || draft.trim() === ''}>
            send
          </button>
        )}
      </div>
      {hint ? <p className="ex-composer__hint">{hint}</p> : null}
    </form>
  );
}

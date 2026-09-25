import { useState } from 'react';
import type { FormEvent, KeyboardEvent } from 'react';

import type { SessionState } from '../session/fold.ts';
import type { Ack } from '../drive/transport.ts';
import './composer.css';

export interface ComposerProps {
  readonly state: SessionState;
  readonly phase: string;
  /** Phases the person may declare a transition to. */
  readonly phases: readonly string[];
  readonly onSend?: (ask: string) => Promise<Ack>;
  readonly onCancel?: () => Promise<Ack>;
  readonly onSeam?: (to: string) => Promise<Ack>;
  /** A line under the input, for a transport that has something to say. */
  readonly hint?: string | undefined;
}

/** What the input says about the session, so a busy drive never looks like a stuck input. */
const STATE_LINE: Readonly<Record<SessionState, string>> = {
  connecting: 'connecting…',
  awaiting: 'your turn',
  turn: 'working · cancel stops it',
  capture: 'interview running · send when it settles',
  ratify: 'ratifying before the refill',
  ended: 'the session has ended',
};

const REFUSED: Readonly<Record<string, string>> = {
  busy: 'not taken: something is still running',
  ended: 'not taken: the session has ended',
  'nothing-to-seam': 'nothing to refill yet: no turn has settled',
  'nothing-to-cancel': 'nothing is running',
  'off-script': 'the canned script expects something else next',
};

export function Composer({ state, phase, phases, onSend, onCancel, onSeam, hint }: ComposerProps) {
  const [draft, setDraft] = useState('');
  const [refusal, setRefusal] = useState<string | undefined>();
  const next = phases[phases.indexOf(phase) + 1] ?? phases.find((p) => p !== phase) ?? phase;
  const [to, setTo] = useState(next);
  const idle = state === 'awaiting';
  const running = state === 'turn' || state === 'capture' || state === 'ratify';

  const answer = (ack: Ack) => setRefusal(ack.ok ? undefined : REFUSED[ack.refused]);

  const send = async (e?: FormEvent) => {
    e?.preventDefault();
    if (!onSend || !idle || draft.trim() === '') return;
    const ack = await onSend(draft.trim());
    answer(ack);
    if (ack.ok) setDraft('');
  };

  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) void send(e);
  };

  return (
    <form className="ex-composer" onSubmit={send} data-state={state}>
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
          {refusal ?? STATE_LINE[state]}
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
          <button type="button" disabled={!idle || !onSeam} onClick={() => onSeam && void onSeam(to).then(answer)}>
            refill
          </button>
        </label>
        {running ? (
          <button type="button" className="ex-composer__cancel" disabled={!onCancel} onClick={() => onCancel && void onCancel().then(answer)}>
            cancel
          </button>
        ) : (
          <button type="submit" className="ex-composer__send" disabled={!idle || !onSend || draft.trim() === ''}>
            send
          </button>
        )}
      </div>
      {hint ? <p className="ex-composer__hint">{hint}</p> : null}
    </form>
  );
}

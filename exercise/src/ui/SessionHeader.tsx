import logoDark from '../../../logo/logo-dark.svg?url';
import logoLight from '../../../logo/logo-light.svg?url';
import type { Link } from '../drive/transport.ts';
import type { Levers, Session } from '../session/fold.ts';
import { Settings } from './Prefs.tsx';
import { Segments } from './Segments.tsx';
import { laneStyle } from './sets.ts';
import type { Room, Surface } from './surface.tsx';
import './header.css';

export interface SessionHeaderProps {
  readonly session: Session;
  readonly link?: Link;
  /** Why the link is not live, when the transport says. */
  readonly linkWhy?: string;
  readonly surface: Surface;
  /** What the row has room for beside the trunk (SessionView): the curtain can draw no more than that. */
  readonly room?: Room;
  readonly onSurface?: (next: Surface) => void;
}

/** How far behind the curtain to look: not at all, each side call as a bar, or every side call whole. */
const CURTAIN = ['closed', 'condensed', 'open'] as const;

/** The levers the header names, in order, and what each state means on hover. */
const LEVERS: readonly { readonly lever: Exclude<keyof Levers, 'table'>; readonly label: string; readonly title: (state: string | undefined) => string }[] = [
  {
    lever: 'approvals',
    label: 'approvals',
    title: (s) =>
      s === 'off'
        ? 'approvals off: every command ran with no gate decision and no prompt; the sandbox still confined it'
        : s === 'gate'
          ? 'the gate decided each command: pre-seeded, approved by the operator, or refused'
          : 'this log does not declare the approval lever',
  },
  { lever: 'forkDelivery', label: 'fork delivery', title: (s) => (s === undefined ? 'this log does not declare how a fork’s result reaches the trunk' : 'how a fork’s result reaches the trunk') },
  { lever: 'reasoning', label: 'reasoning', title: (s) => (s === undefined ? 'this log does not declare the reasoning state it sent' : 'the reasoning state sent on every request') },
];

/** One quiet line: what is running this session, where it is, and the switches for seeing more. */
export function SessionHeader({ session, link = 'live', linkWhy, surface, room = 'whole', onSurface }: SessionHeaderProps) {
  const unavailable = CURTAIN.filter((c) => (room === 'bars' && c === 'open') || (room === 'none' && c !== 'closed'));
  const drawn = !surface.curtain || room === 'none' ? 'closed' : surface.condensed === true || room === 'bars' ? 'condensed' : 'open';
  return (
    <header className="ex-header">
      {/* The repository's logo (logo/), as it ships: one file per mode, the one the surface's own mode asks for shown. */}
      <span className="ex-header__logo">
        <img className="ex-header__logo-img" data-for="dark" src={logoDark} alt="Discipline" />
        <img className="ex-header__logo-img" data-for="light" src={logoLight} alt="Discipline" />
      </span>
      {/* What the log has not said is left out: `diet`'s v0 names no arm and no phase. */}
      {session.arm ? (
        <span className="ex-header__item">
          <span className="ex-header__k">arm</span> {session.arm}
        </span>
      ) : null}
      <span className="ex-header__item ex-header__model">{session.model}</span>
      {/* Every lever's state, as the session's first line declares the table (#623): the whole of it, on asking. */}
      {session.levers.table ? (
        <details className="ex-header__item ex-header__levers">
          <summary title="every lever's state this session ran at, as its first line declares them">levers</summary>
          <dl>
            {Object.entries(session.levers.table).map(([lever, state]) => (
              <div key={lever} data-lever-row={lever} data-undeclared={state === 'undeclared' ? '' : undefined}>
                <dt>{lever.replaceAll('_', ' ')}</dt>
                <dd>{state}</dd>
              </div>
            ))}
          </dl>
        </details>
      ) : null}
      {/* The lever states the session ran under, as its first line declares them (#573); what it does not declare says so. */}
      {LEVERS.map(({ lever, label, title }) => (
        <span key={lever} className="ex-header__item ex-header__lever" data-lever={lever} data-undeclared={session.levers[lever] === undefined ? '' : undefined} title={title(session.levers[lever])}>
          <span className="ex-header__k">{label}</span> {session.levers[lever] ?? 'undeclared'}
        </span>
      ))}
      {session.phase ? (
        <span className="ex-header__item">
          <span className="ex-header__k">phase</span> {session.phase}
        </span>
      ) : null}
      <span className="ex-header__slots" title="the server's slots, and what each is serving now">
        {session.occupancy.map((holder, slot) => (
          <span
            key={slot}
            className="ex-header__slot"
            data-busy={holder === undefined ? undefined : ''}
            data-lane={holder?.lane}
            style={laneStyle(holder?.lane)}
            title={holder !== undefined ? `slot ${slot}: ${holder.lane} ${holder.id}` : `slot ${slot}: idle`}
          >
            <span className="ex-header__dot" aria-hidden="true" />
            {slot}
            {slot === session.trunkSlot ? '·trunk' : ''}
            {holder !== undefined && slot !== session.trunkSlot ? <span className="ex-header__holder"> {holder.lane}</span> : null}
          </span>
        ))}
      </span>
      <span className="ex-header__spacer" />
      {surface.curtain && session.unknown.size > 0 ? (
        <span
          className="ex-header__unknown"
          title={`events of a kind this surface does not draw yet, kept in the log: ${[...session.unknown].map(([kind, n]) => `${kind} ×${n}`).join(', ')}`}
        >
          {[...session.unknown.values()].reduce((a, b) => a + b, 0)} unknown
        </span>
      ) : null}
      {link !== 'live' ? (
        <span className="ex-header__link" data-link={link} role="status" title={linkWhy}>
          {link === 'reconnecting' ? 'reconnecting…' : 'connection lost'}
          {link === 'lost' && linkWhy ? `: ${linkWhy}` : ''}
        </span>
      ) : null}
      <span className="ex-header__state" data-state={session.state}>
        {session.state}
      </span>
      {onSurface ? (
        <>
          <span className="ex-header__toggle">
            curtain
            <Segments
              name="curtain"
              options={CURTAIN}
              value={drawn}
              onPick={(next) => onSurface({ ...surface, curtain: next !== 'closed', condensed: next === 'condensed' })}
              unavailable={unavailable}
              why="no room beside the trunk at this width: a side call opens under its message"
            />
          </span>
          <label className="ex-header__toggle">
            <input type="checkbox" checked={surface.gaps} onChange={(e) => onSurface({ ...surface, gaps: e.target.checked })} />
            what diet can’t emit yet
          </label>
        </>
      ) : null}
      <Settings />
    </header>
  );
}

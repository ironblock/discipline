import type { Link } from '../drive/transport.ts';
import type { Session } from '../session/fold.ts';
import { Settings } from './Prefs.tsx';
import { Segments } from './Segments.tsx';
import { laneStyle } from './sets.ts';
import type { Room, Surface } from './surface.tsx';
import './header.css';

export interface SessionHeaderProps {
  readonly session: Session;
  readonly link?: Link;
  readonly surface: Surface;
  /** What the row has room for beside the trunk (SessionView): the curtain can draw no more than that. */
  readonly room?: Room;
  readonly onSurface?: (next: Surface) => void;
}

/** How far behind the curtain to look: not at all, each side call as a bar, or every side call whole. */
const CURTAIN = ['closed', 'condensed', 'open'] as const;

/** One quiet line: what is running this session, where it is, and the switches for seeing more. */
export function SessionHeader({ session, link = 'live', surface, room = 'whole', onSurface }: SessionHeaderProps) {
  const unavailable = CURTAIN.filter((c) => (room === 'bars' && c === 'open') || (room === 'none' && c !== 'closed'));
  const drawn = !surface.curtain || room === 'none' ? 'closed' : surface.condensed === true || room === 'bars' ? 'condensed' : 'open';
  return (
    <header className="ex-header">
      <span className="ex-header__item">
        <span className="ex-header__k">arm</span> {session.arm}
      </span>
      <span className="ex-header__item ex-header__model">{session.model}</span>
      <span className="ex-header__item">
        <span className="ex-header__k">phase</span> {session.phase}
      </span>
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
        <span className="ex-header__link" data-link={link} role="status">
          {link === 'reconnecting' ? 'reconnecting…' : 'connection lost'}
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

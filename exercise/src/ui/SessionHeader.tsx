import type { Link } from '../drive/transport.ts';
import type { Session } from '../session/fold.ts';
import { laneStyle } from './sets.ts';
import type { Surface } from './surface.tsx';
import './header.css';

export interface SessionHeaderProps {
  readonly session: Session;
  readonly link?: Link;
  readonly surface: Surface;
  readonly onSurface?: (next: Surface) => void;
}

/** One quiet line: what is running this session, where it is, and the switches for seeing more. */
export function SessionHeader({ session, link = 'live', surface, onSurface }: SessionHeaderProps) {
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
          >
            <span className="ex-header__dot" aria-hidden="true" />
            {slot}
            {slot === session.trunkSlot ? '·trunk' : ''}
            {holder !== undefined ? <span className="ex-header__holder"> {holder.id}</span> : null}
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
          <label className="ex-header__toggle">
            <input type="checkbox" checked={surface.curtain} onChange={(e) => onSurface({ ...surface, curtain: e.target.checked })} />
            behind the curtain
          </label>
          <label className="ex-header__toggle">
            <input type="checkbox" checked={surface.gaps} onChange={(e) => onSurface({ ...surface, gaps: e.target.checked })} />
            what diet can’t emit yet
          </label>
        </>
      ) : null}
    </header>
  );
}

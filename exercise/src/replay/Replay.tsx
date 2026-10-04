import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';

import type { FileSource } from '../drive/files.ts';
import { ReplayTransport } from '../drive/recorded.ts';
import type { Recording } from '../drive/recorded.ts';
import { useSession } from '../session/useSession.ts';
import { PHASES } from '../ui/phases.ts';
import { SessionView } from '../ui/SessionView.tsx';
import type { Surface } from '../ui/surface.tsx';
import { EXAMPLE_LABEL, EXAMPLES, NOT_PUBLISHED, PUBLISHED } from './published.ts';
import './replay.css';

/** What the page does not show yet, and whose it is to add (#32, rulings 5-7). */
const GAPS = [
  'Working memory at any turn, and the receipt beside the floor’s, are the surface’s to add (#31).',
  'The first driven session under #117 is not recorded yet; it will be a results directory (#177).',
  'Reading a diet log in the browser waits on diet’s own reader (#117), so these are migrated recordings, not logs.',
];

/** The page's frame, on every page: where it is, what it does not show yet and whose that is, the licence. */
function Frame({ children }: { readonly children?: React.ReactNode }) {
  return (
    <div className="ex-replay">
      <header className="ex-replay__head">
        <a href="../">discipline</a> · replay
      </header>
      {children}
      <section className="ex-replay__gaps" aria-label="not here yet">
        <h2>Not here yet</h2>
        <ul className="ex-replay__list">
          {GAPS.map((gap) => (
            <li key={gap}>{gap}</li>
          ))}
        </ul>
      </section>
      <footer className="ex-replay__foot">
        <a href="https://github.com/ironblock/discipline">github.com/ironblock/discipline</a> · Apache-2.0
      </footer>
    </div>
  );
}

/**
 * The maintainer's sentence, over an authored example (#272): beside its name
 * in the index, and held at the top of its replay from the first event to the
 * last, so no moment of it reads as a session.
 */
function ExampleLabel() {
  return (
    <p className="ex-replay__label" role="note">
      {EXAMPLE_LABEL}
    </p>
  );
}

/**
 * The label pinned over a replay: the page keeps its height clear above the session's own pinned chrome
 * (`--ex-pinned-top`, session.css), so the label covers none of it, at any width the sentence wraps to.
 */
function PinnedExampleLabel() {
  const label = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const el = label.current;
    const page = el?.parentElement;
    if (!el || !page) return;
    const keep = () => page.style.setProperty('--ex-pinned-top', `${el.getBoundingClientRect().height}px`);
    keep();
    const watch = new ResizeObserver(keep);
    watch.observe(el);
    return () => {
      watch.disconnect();
      page.style.removeProperty('--ex-pinned-top');
    };
  }, []);
  return (
    <div ref={label} className="ex-replay__pinned">
      <ExampleLabel />
    </div>
  );
}

/** Why a recording is not published, if this page says. An own key only: `?session=constructor` is not a reason. */
const whyNot = (name: string) => (Object.hasOwn(NOT_PUBLISHED, name) ? NOT_PUBLISHED[name] : undefined);

/**
 * No recording named, one the page does not publish, or `?drive`: what it does publish, and what it does not
 * and why. The authored examples are a section of their own, under their label, never among the recordings
 * (#272). Driving is local (`diet serve`); this page replays, and says so, naming the landing page.
 */
export function ReplayIndex({ asked, drive = false }: { readonly asked?: string | undefined; readonly drive?: boolean }) {
  const why = asked !== undefined ? whyNot(asked) : undefined;
  return (
    <Frame>
      {drive ? (
        <p className="ex-replay__note">
          This page replays; it does not drive. Driving is local, against <code>diet serve</code>. The project's landing page is <a href="../">discipline</a>.
        </p>
      ) : null}
      {asked !== undefined ? <p className="ex-replay__note">Not published here: {asked}.{why ? ` It is not published because ${why}.` : ''}</p> : null}
      <section className="ex-replay__recordings" aria-label="recorded sessions">
        <h1>Recorded sessions</h1>
        <ul className="ex-replay__list">
          {PUBLISHED.map((name) => (
            <li key={name}>
              <a href={`?session=${name}`}>{name}</a>
            </li>
          ))}
        </ul>
      </section>
      <section className="ex-replay__examples" aria-label="authored examples">
        <h2>Authored examples (not sessions)</h2>
        <ExampleLabel />
        <ul className="ex-replay__list">
          {EXAMPLES.map((name) => (
            <li key={name}>
              <a href={`?session=${name}`}>{name}</a>
            </li>
          ))}
        </ul>
      </section>
      <h2>Not published</h2>
      <ul className="ex-replay__list">
        {Object.entries(NOT_PUBLISHED).map(([name, why]) => (
          <li key={name}>
            {name}: {why}
          </li>
        ))}
      </ul>
    </Frame>
  );
}

/**
 * One recording, replayed: its source and scrub drawn above it (the migration's own header), then the session.
 * An authored example (#272) is replayed the same way under its label, which stays on the page throughout.
 */
export function Replay({
  name,
  recording,
  speed = 1,
  example = false,
  files,
}: {
  readonly name: string;
  readonly recording: Recording;
  readonly speed?: number;
  readonly example?: boolean;
  /** The recording's published files by digest (#372). */
  readonly files?: FileSource;
}) {
  const transport = useMemo(() => new ReplayTransport(recording, { speed, ...(files ? { files } : {}) }), [recording, speed, files]);
  useEffect(() => () => transport.close(), [transport]);
  const session = useSession(transport);
  const [surface, setSurface] = useState<Surface>({ curtain: true, gaps: false });
  return (
    <Frame>
      {example ? <PinnedExampleLabel /> : null}
      <section className="ex-replay__source" aria-label="where this recording came from">
        <h1>{recording.title}</h1>
        <details>
          <summary>
            {name}: {example ? 'how it was authored' : 'how it was recorded, migrated and scrubbed'} ({recording.migration.length} lines)
          </summary>
          <ul>
            {recording.migration.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
        </details>
      </section>
      <SessionView
        session={session}
        surface={surface}
        onSurface={setSurface}
        follow
        {...(transport.file ? { files: transport.file } : {})}
        composer={{ phases: PHASES, dispatch: () => transport.dispatch(), hint: `replaying: ${recording.title}` }}
      />
    </Frame>
  );
}

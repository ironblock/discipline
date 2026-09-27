import { useEffect, useMemo, useState } from 'react';

import { CannedTransport } from './drive/canned.ts';
import { RECORDINGS, ReplayTransport } from './drive/recorded.ts';
import type { RecordingName } from './drive/recorded.ts';
import { SPECIMEN } from './drive/specimen.ts';
import type { DriveTransport, Link } from './drive/transport.ts';
import { useSession } from './session/useSession.ts';
import { SessionView } from './ui/SessionView.tsx';
import type { Surface } from './ui/surface.tsx';

/**
 * The phases a person may move between. The predecessor's list, until the
 * regimen's phase graph reaches the surface with the seam (#117 R6).
 */
export const PHASES = ['orient', 'spec', 'plan', 'build', 'review'] as const;

const EXPECTS: Readonly<Record<string, string>> = {
  send: 'canned: the script expects an ask next (type anything; the model side is scripted)',
  seam: 'canned: the script expects a refill next (move to build)',
};

/**
 * The harness, on the canned transport until #117's loop serves a real one --
 * or replaying a recorded session, which plays and takes no commands.
 */
export function App({ speed = 1, recording }: { readonly speed?: number; readonly recording?: RecordingName }) {
  const transport = useMemo(
    () => (recording ? new ReplayTransport(RECORDINGS[recording], { speed }) : new CannedTransport(SPECIMEN, { speed })),
    [speed, recording],
  );
  useEffect(() => () => transport.close(), [transport]);
  const session = useSession(transport);
  const [link, setLink] = useState<Link>('live');
  useEffect(() => (transport as DriveTransport).watchLink?.(setLink), [transport]);
  const [surface, setSurface] = useState<Surface>({ curtain: true, gaps: false });
  const expects = transport instanceof CannedTransport ? transport.expects : undefined;
  return (
    <SessionView
      session={session}
      link={link}
      surface={surface}
      onSurface={setSurface}
      follow
      composer={{
        phases: PHASES,
        dispatch: (command) => transport.dispatch(command),
        hint: recording
          ? `recorded: ${RECORDINGS[recording].title}`
          : session.state === 'awaiting'
            ? expects
              ? EXPECTS[expects]
              : 'canned: the script has ended'
            : undefined,
      }}
    />
  );
}

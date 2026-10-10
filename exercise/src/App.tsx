import { useEffect, useMemo, useState } from 'react';

import { CannedTransport } from './drive/canned.ts';
import { HttpTransport } from './drive/http.ts';
import type { Web } from './drive/http.ts';
import { ReplayTransport } from './drive/recorded.ts';
import { SESSIONS } from './drive/sessions.ts';
import type { SessionName } from './drive/sessions.ts';
import { SPECIMEN } from './drive/specimen.ts';
import type { Beat } from './drive/specimen.ts';
import type { Command, Decision, DriveTransport, Link, Prompt } from './drive/transport.ts';
import { useIdleGap } from './session/useIdleGap.ts';
import { useSession } from './session/useSession.ts';
import { SessionView } from './ui/SessionView.tsx';
import type { Surface } from './ui/surface.tsx';

export { PHASES } from './ui/phases.ts';
import { PHASES } from './ui/phases.ts';

const EXPECTS: Readonly<Record<string, string>> = {
  send: 'canned: the script expects an ask next (type anything; the model side is scripted)',
  seam: 'canned: the script expects a refill next (move to build)',
};

/**
 * The harness: driving `diet`'s session over HTTP (`drive`, #117 I5), or on
 * the canned transport, or replaying a recorded session, which plays and
 * takes no commands. `web` stands in for the browser's own `EventSource` and
 * `fetch` while driving, so a story can serve the page a log (#288); `script`
 * is what the canned transport plays, the specimen unless a story says.
 */
export function App({
  speed = 1,
  recording,
  drive = false,
  web,
  script = SPECIMEN,
}: {
  readonly speed?: number;
  readonly recording?: SessionName;
  readonly drive?: boolean;
  readonly web?: Web;
  readonly script?: readonly Beat[];
}) {
  const transport = useMemo(
    () => (drive ? new HttpTransport('', web) : recording ? new ReplayTransport(SESSIONS[recording], { speed }) : new CannedTransport(script, { speed })),
    [speed, recording, drive, web, script],
  );
  useEffect(() => () => transport.close(), [transport]);
  const session = useSession(transport);
  // The idle gap a settled turn opens, carried by the command that ends it (Q4). `diet` logs it only if that
  // command is admitted and drops it if refused (#146): the gap ends when the command is admitted, and a send
  // refused because work was in flight blocks the person from there.
  const gap = useIdleGap(session);
  const dispatch = async (command: Command) => {
    // An answer to a prompt and a move to the background (#614) come mid-turn, and a tangent's open or close (#608) is no
    // `GapEnd`: none of them ends a gap.
    if (command.kind === 'approve' || command.kind === 'background' || command.kind === 'ratify-phase' || command.kind === 'open-tangent' || command.kind === 'close-tangent') return transport.dispatch(command);
    // A command's kind is the word for what it ends the gap with (the format's `GapEnd`): ask, seam, cancel, end.
    const idleGap = gap.carry(command.kind);
    const ack = await transport.dispatch(command, idleGap ? { idle_gap: idleGap } : undefined);
    if (ack.ok) gap.admitted();
    else if (ack.refused === 'in-flight' || ack.refused === 'busy') gap.refused();
    return ack;
  };
  const [waiting, setWaiting] = useState<Prompt | undefined>(undefined);
  useEffect(() => (transport as DriveTransport).watchPrompt?.(setWaiting), [transport]);
  const decide = (call: string, scope: Decision) => transport.dispatch({ kind: 'approve', call, scope });
  const [link, setLink] = useState<{ readonly link: Link; readonly why?: string }>({ link: 'live' });
  useEffect(() => (transport as DriveTransport).watchLink?.((next, why) => setLink(why === undefined ? { link: next } : { link: next, why })), [transport]);
  const [surface, setSurface] = useState<Surface>({ curtain: true, gaps: false });
  const expects = transport instanceof CannedTransport ? transport.expects : undefined;
  // The operator's PNGs go here ahead of the ask (#372); a replay takes none.
  const uploader = (transport as DriveTransport).upload;
  return (
    <SessionView
      session={session}
      link={link.link}
      {...(link.why !== undefined ? { linkWhy: link.why } : {})}
      surface={surface}
      onSurface={setSurface}
      follow
      {...(transport.file ? { files: transport.file } : {})}
      approving={{ waiting, decide }}
      composer={{
        // The canned script's phases; under `?drive`, the moves the logged phase graph allows from where the session is (#563).
        phases: drive ? session.phaseMoves : PHASES,
        dispatch,
        // Tangents are the served drive's (#608): the canned script plays none.
        tangents: drive === true,
        ...(uploader ? { upload: (bytes: Uint8Array) => uploader.call(transport, bytes) } : {}),
        hint: drive
          ? `driving: diet's session, over HTTP`
          : recording
          ? `replaying: ${SESSIONS[recording].title}`
          : session.state === 'awaiting'
            ? expects
              ? EXPECTS[expects]
              : 'canned: the script has ended'
            : undefined,
      }}
    />
  );
}

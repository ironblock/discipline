import { useEffect, useMemo, useState } from 'react';

import type { LogLine } from '../drive/log.ts';
import type { DriveTransport } from '../drive/transport.ts';
import { fold } from './fold.ts';
import type { Session } from './fold.ts';

/** Subscribe to a transport's log and fold it. Refolds per event; a session is small. */
export function useSession(transport: DriveTransport): Session {
  const [events, setEvents] = useState<readonly LogLine[]>([]);
  useEffect(() => {
    const seen: LogLine[] = [];
    setEvents([]);
    return transport.subscribe((line) => {
      // A session's first line after another's: the drive restarted, and this is a new session.
      if (line.seq === 0) seen.length = 0;
      seen.push(line);
      setEvents([...seen]);
    });
  }, [transport]);
  return useMemo(() => fold(events), [events]);
}

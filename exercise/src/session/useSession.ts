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
    return transport.subscribe((event) => {
      seen.push(event);
      setEvents([...seen]);
    });
  }, [transport]);
  return useMemo(() => fold(events), [events]);
}

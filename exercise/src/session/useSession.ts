import { useEffect, useMemo, useState } from 'react';

import type { DriveEvent } from '../drive/events.ts';
import type { DriveTransport } from '../drive/transport.ts';
import { fold } from './fold.ts';
import type { Session } from './fold.ts';

/** Subscribe to a transport's log and fold it. Refolds per event; a session is small. */
export function useSession(transport: DriveTransport): Session {
  const [events, setEvents] = useState<readonly DriveEvent[]>([]);
  useEffect(() => {
    const seen: DriveEvent[] = [];
    setEvents([]);
    return transport.subscribe((event) => {
      seen.push(event);
      setEvents([...seen]);
    });
  }, [transport]);
  return useMemo(() => fold(events), [events]);
}

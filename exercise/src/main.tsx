import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './App.tsx';
import { SESSIONS } from './drive/sessions.ts';
import type { SessionName } from './drive/sessions.ts';
import './theme/tokens.css';
import './theme/themes/index.ts';
import { PREFS } from './ui/prefs.ts';
import { Preferred } from './ui/Prefs.tsx';

// `?speed=4` plays the session four times as fast; `?session=first-drive`
// replays a recorded session instead of the canned one (`kitchen-sink`, an
// authored one at a working drive's cadence). Any preference may be
// set for one visit by name (`?theme=paper&mode=light&connectors=sweep`,
// prefs.ts), over what the settings remember.
const params = new URLSearchParams(window.location.search);
const speed = Number(params.get('speed') ?? '1') || 1;
const session = params.get('session');
const recording = session !== null && Object.hasOwn(SESSIONS, session) ? (session as SessionName) : undefined;
const overrides = Object.fromEntries(Object.keys(PREFS).map((name) => [name, params.get(name)]));

const root = document.getElementById('root');
if (!root) throw new Error('index.html has no #root');

createRoot(root).render(
  <StrictMode>
    <Preferred overrides={overrides}>
      <App speed={speed} {...(recording ? { recording } : {})} />
    </Preferred>
  </StrictMode>,
);

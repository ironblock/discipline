import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './App.tsx';
import { RECORDINGS } from './drive/recorded.ts';
import type { RecordingName } from './drive/recorded.ts';
import './theme/tokens.css';
import { isTheme } from './theme/themes/index.ts';
import { Looked } from './ui/look.tsx';
import type { Surface } from './ui/surface.tsx';

// `?speed=4` plays the session four times as fast; `?theme=lantern` tries a lab
// theme (until a look is picked in the header);
// `?session=first-drive` replays a recorded session instead of the canned one;
// the lines are routed as a harness whose crossings hop: `?wiring=gap` breaks
// them instead, `?wiring=curves` draws curves, `&bend=chamfer` (or `square`)
// cuts the harness's corners so.
const params = new URLSearchParams(window.location.search);
const speed = Number(params.get('speed') ?? '1') || 1;
const session = params.get('session');
const recording = session !== null && Object.hasOwn(RECORDINGS, session) ? (session as RecordingName) : undefined;
const requested = params.get('theme');
const pinned = isTheme(requested) ? requested : undefined;
const crossing = params.get('wiring');
const bend = params.get('bend');
const wiring: Surface['wiring'] =
  crossing === 'curves' ? undefined : { crossing: crossing === 'gap' ? 'gap' : 'hop', bend: bend === 'chamfer' || bend === 'square' ? bend : 'round' };

const root = document.getElementById('root');
if (!root) throw new Error('index.html has no #root');

createRoot(root).render(
  <StrictMode>
    <Looked pinned={pinned}>
      <App speed={speed} {...(recording ? { recording } : {})} {...(wiring ? { wiring } : {})} />
    </Looked>
  </StrictMode>,
);

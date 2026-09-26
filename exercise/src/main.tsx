import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './App.tsx';
import { RECORDINGS } from './drive/recorded.ts';
import type { RecordingName } from './drive/recorded.ts';
import './theme/tokens.css';
import { DEFAULT_THEME, isTheme, layersOf } from './theme/themes/index.ts';

// `?speed=4` plays the session four times as fast; `?theme=paper` tries a theme;
// `?session=first-drive` replays a recorded session instead of the canned one.
const params = new URLSearchParams(window.location.search);
const speed = Number(params.get('speed') ?? '1') || 1;
const session = params.get('session');
const recording = session !== null && Object.hasOwn(RECORDINGS, session) ? (session as RecordingName) : undefined;
const requested = params.get('theme');
const theme = isTheme(requested) ? requested : DEFAULT_THEME;

const root = document.getElementById('root');
if (!root) throw new Error('index.html has no #root');

createRoot(root).render(
  <StrictMode>
    <div className="ex-root" data-theme={layersOf(theme)}>
      <App speed={speed} {...(recording ? { recording } : {})} />
    </div>
  </StrictMode>,
);

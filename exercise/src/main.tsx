import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './App.tsx';
import './theme/tokens.css';
import { isTheme, layersOf } from './theme/themes/index.ts';

// `?speed=4` plays the canned session four times as fast; `?theme=paper` tries a theme.
const params = new URLSearchParams(window.location.search);
const speed = Number(params.get('speed') ?? '1') || 1;
const requested = params.get('theme');
const theme = isTheme(requested) ? requested : 'mockup';

const root = document.getElementById('root');
if (!root) throw new Error('index.html has no #root');

createRoot(root).render(
  <StrictMode>
    <div className="ex-root" data-theme={layersOf(theme)}>
      <App speed={speed} />
    </div>
  </StrictMode>,
);

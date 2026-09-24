import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './App.tsx';
import './theme/tokens.css';

// `?speed=4` plays the canned session four times as fast.
const speed = Number(new URLSearchParams(window.location.search).get('speed') ?? '1') || 1;

const root = document.getElementById('root');
if (!root) throw new Error('index.html has no #root');

createRoot(root).render(
  <StrictMode>
    <div className="ex-root">
      <App speed={speed} />
    </div>
  </StrictMode>,
);

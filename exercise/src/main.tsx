import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './App.tsx';
import { driveBase } from './drive/base.ts';
import { credentialedWeb, shellCredential } from './drive/credential.ts';
import { shellPage } from './drive/shellPage.ts';
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
// `?drive`: `diet`'s session (`diet-drive serve`). Same-origin by default: the page served by serve itself (`--ui`,
// built by `build:drive`), or by Vite's dev server through its proxy (`DIET_DRIVE`). `<meta name="diet-drive">`
// (Tauri's, or anyone serving the page apart from serve) or `?drive=<url>` names another drive (#716).
const { drive, base } = driveBase(document, window.location.search);
const recording = session !== null && Object.hasOwn(SESSIONS, session) ? (session as SessionName) : undefined;
const overrides = Object.fromEntries(Object.keys(PREFS).map((name) => [name, params.get(name)]));

const root = document.getElementById('root');
if (!root) throw new Error('index.html has no #root');

/** Say TEXT in the window in place of the app: the shell's start, or why it has no drive (the app audit, finding 6). */
function say(text: string, role: 'status' | 'alert'): void {
  const said = document.createElement('p');
  said.setAttribute('role', role);
  said.className = 'ex-shell-page';
  said.textContent = text;
  root!.replaceChildren(said);
}

const shell = shellPage(window.location.search);
if (shell?.kind === 'starting') say('starting serve…', 'status');
else if (shell?.kind === 'failed') say(shell.why, 'alert');
else await mount();

/** The app, on its drive; or, when the shell will not hand over the credential, why. */
async function mount(): Promise<void> {
  // The native shell holds the drive's credential and hands it over here, never in a URL (#719, `drive/credential.ts`);
  // in a browser there is none, and Vite's proxy presents it. A shell that will not hand it over is said so (the app audit).
  const credential = drive ? await shellCredential() : {};
  if ('why' in credential) return say(credential.why, 'alert');
  const web = credential.authorization ? credentialedWeb(credential.authorization) : undefined;

  createRoot(root!).render(
    <StrictMode>
      <Preferred overrides={overrides}>
        <App speed={speed} drive={drive} base={base} {...(web ? { web } : {})} {...(recording ? { recording } : {})} />
      </Preferred>
    </StrictMode>,
  );
}

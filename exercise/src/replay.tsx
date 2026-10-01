import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { load } from './drive/recorded.ts';
import { isPublished } from './replay/published.ts';
import { Replay, ReplayIndex } from './replay/Replay.tsx';
import './theme/tokens.css';
import './theme/themes/index.ts';
import { PREFS } from './ui/prefs.ts';
import { Preferred } from './ui/Prefs.tsx';

/**
 * The replay page (#32): a recorded session, from this origin, with nothing
 * else -- no drive, no endpoint, no network but this site's own files. The
 * recording is not bundled: each is a file of its own beside the page,
 * `data/<name>.js` (`export default` and the recording verbatim), loaded at
 * run time from the page's base. Never a specifier relative to this module,
 * which the build puts under `assets/`, where no recording is.
 */
const params = new URLSearchParams(window.location.search);
const asked = params.get('session');
const speed = Number(params.get('speed') ?? '1') || 1;
const overrides = Object.fromEntries(Object.keys(PREFS).map((name) => [name, params.get(name)]));

/** Where a published recording's file is: beside the page, under its base. */
export const payloadUrl = (name: string) => new URL(`${import.meta.env.BASE_URL}data/${name}.js`, document.baseURI).href;

async function recordingOf(name: string) {
  const url = payloadUrl(name);
  const module = (await import(/* @vite-ignore */ url)) as { readonly default: unknown };
  return load(name, JSON.stringify(module.default));
}

const root = document.getElementById('root');
if (!root) throw new Error('replay.html has no #root');
const mount = createRoot(root);
const show = (page: React.ReactNode) =>
  mount.render(
    <StrictMode>
      <Preferred overrides={overrides}>{page}</Preferred>
    </StrictMode>,
  );

if (params.has('drive')) {
  // Drive mode stays local (`diet serve`): this page cannot reach a box, and does not try.
  show(<ReplayIndex asked="?drive (driving is local only: run the harness against diet serve)" />);
} else if (!isPublished(asked)) {
  show(<ReplayIndex {...(asked !== null ? { asked } : {})} />);
} else {
  recordingOf(asked).then(
    (recording) => show(<Replay name={asked} recording={recording} speed={speed} />),
    (err: unknown) => {
      root.setAttribute('data-failed', '');
      root.textContent = `could not load ${payloadUrl(asked)}: ${(err as Error).message}`;
    },
  );
}

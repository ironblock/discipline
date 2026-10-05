import type { Meta, StoryObj } from '@storybook/react-vite';
import { useEffect, useMemo } from 'react';
import { expect, userEvent, waitFor } from 'storybook/test';

import { PHASES } from '../App.tsx';
import { CannedTransport, snapshot } from '../drive/canned.ts';
import { assetModule, importedFiles } from '../drive/files.ts';
import type { FileSource } from '../drive/files.ts';
import { SCENE_PNG, SCENE_SHA256, SCREENSHOT } from '../drive/screenshot.ts';
import { fold } from '../session/fold.ts';
import { useSession } from '../session/useSession.ts';
import { SessionView } from '../ui/SessionView.tsx';

/**
 * A tool call whose result is a file (#372): T1's screenshot, carried in the
 * log by reference and drawn from its bytes once they hash to its digest --
 * from `serve` live, from the recording's published asset in a replay -- and
 * what is said instead when they cannot be: withheld, not found, or not the
 * bytes the log pins.
 */
const meta = {
  title: 'Session/Files',
  parameters: { layout: 'fullscreen' },
} satisfies Meta;

export default meta;

const surface = { curtain: true, gaps: false } as const;
const settled = fold(snapshot(SCREENSHOT, { beat: SCREENSHOT.length }));
const shot = (root: HTMLElement) => root.querySelector<HTMLElement>('.ex-trunk [data-tone="tool"] .ex-file');

/** The session as its log has it, read through SOURCE. */
function Logged({ source }: { readonly source?: FileSource }) {
  return <SessionView session={settled} surface={surface} composer={{ phases: PHASES }} {...(source ? { files: source } : {})} />;
}

/** The canned drive playing the screenshot session, its files served by digest as `serve` would. */
function Driven() {
  const transport = useMemo(() => new CannedTransport(SCREENSHOT, { speed: 40 }), []);
  useEffect(() => () => transport.close(), [transport]);
  const session = useSession(transport);
  return <SessionView session={session} surface={surface} follow files={transport.file} composer={{ phases: PHASES, dispatch: (command) => transport.dispatch(command) }} />;
}

/** The image, drawn: loaded, at its own size, from a blob of the checked bytes -- never from its path. */
async function drawn(root: HTMLElement) {
  const image = await waitFor(async () => {
    const found = shot(root)?.querySelector<HTMLImageElement>('img.ex-file__image');
    await expect(found).not.toBeNull();
    await expect(found!.complete && found!.naturalWidth).toBe(160);
    return found!;
  });
  await expect(image.src.startsWith('blob:')).toBe(true);
  await expect(image.dataset['sha256']).toBe(SCENE_SHA256);
  await expect(shot(root)?.querySelector('.ex-file__ref')?.textContent).toBe(`shots/scene.png · image/png · ${(SCENE_PNG.length / 1024).toFixed(1)} KB · ${SCENE_SHA256.slice(0, 12)}`);
}

/** What is said in place of an image that is not drawn. */
const unshown = (root: HTMLElement) => shot(root)?.querySelector('.ex-file__unshown')?.textContent;

export const Live: StoryObj = {
  name: 'live: the screenshot, read by digest and drawn',
  render: () => <Driven />,
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(canvasElement.querySelector('.ex-composer__state')?.textContent).toBe('your turn'));
    await userEvent.type(canvasElement.querySelector('textarea') as HTMLTextAreaElement, 'Render the scene and show me a screenshot.{Enter}');
    await drawn(canvasElement);
  },
};

export const Replayed: StoryObj = {
  name: 'replayed: the recording’s published asset, read with import()',
  render: () => <Logged source={importedFiles((sha256) => (sha256 === SCENE_SHA256 ? `data:text/javascript;base64,${btoa(assetModule(SCENE_PNG))}` : 'data:text/javascript,'))} />,
  play: async ({ canvasElement }) => drawn(canvasElement),
};

export const NotTheBytes: StoryObj = {
  name: 'bytes that are not the digest: refused, never drawn',
  render: () => <Logged source={() => Promise.resolve({ kind: 'bytes', bytes: SCENE_PNG.slice(0, -1) })} />,
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(unshown(canvasElement)).toMatch(new RegExp(`^refused: the bytes are not ${SCENE_SHA256} \\(they are [0-9a-f]{64}\\)$`)));
    await expect(shot(canvasElement)?.querySelector('img')).toBeNull();
  },
};

export const NotFound: StoryObj = {
  name: 'a digest the source has no file for: a refusal naming it',
  render: () => <Logged source={() => Promise.resolve({ kind: 'not-found' })} />,
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(unshown(canvasElement)).toBe(`refused: no file ${SCENE_SHA256}`));
  },
};

export const Withheld: StoryObj = {
  name: 'withheld from the publication: the reference, and the word',
  render: () => <Logged source={() => Promise.resolve({ kind: 'withheld' })} />,
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(unshown(canvasElement)).toBe('withheld: its recording does not declare it clean, so it is not published'));
    await expect(shot(canvasElement)?.querySelector('.ex-file__ref')?.textContent).toContain('shots/scene.png');
  },
};

/** The operator's screenshot on the ask (#372, log v5's `ask.files`, #458): the same session, the scene attached to its ask. */
const ref = { path: 'shots/scene.png', sha256: SCENE_SHA256, media_type: 'image/png', bytes: SCENE_PNG.length };
const attached = fold(snapshot(SCREENSHOT, { beat: SCREENSHOT.length }).map((line) => (line.kind === 'ask' ? { ...line, files: [ref] } : line)));
const asked = (root: HTMLElement) => root.querySelector<HTMLElement>('.ex-trunk [data-tone="user"] .ex-file');

/** The operator's attachment on their ask, drawn from its checked bytes: as `serve`'s GET /files answers, or a replay's asset. */
async function drawnOnTheAsk(root: HTMLElement) {
  const image = await waitFor(async () => {
    const found = asked(root)?.querySelector<HTMLImageElement>('img.ex-file__image');
    await expect(found).not.toBeNull();
    await expect(found!.complete && found!.naturalWidth).toBe(160);
    return found!;
  });
  await expect(image.src.startsWith('blob:')).toBe(true);
  await expect(image.dataset['sha256']).toBe(SCENE_SHA256);
  await expect(root.querySelector('.ex-trunk [data-tone="user"] .ex-files')?.getAttribute('aria-label')).toBe('what the operator attached');
}

export const AskAttachedLive: StoryObj = {
  name: 'the operator’s screenshot on their ask, live: read by digest and drawn',
  render: () => <SessionView session={attached} surface={surface} composer={{ phases: PHASES }} files={() => Promise.resolve({ kind: 'bytes', bytes: SCENE_PNG })} />,
  play: async ({ canvasElement }) => drawnOnTheAsk(canvasElement),
};

export const AskAttachedReplayed: StoryObj = {
  name: 'the operator’s screenshot on their ask, replayed from the recording’s asset',
  render: () => (
    <SessionView
      session={attached}
      surface={surface}
      composer={{ phases: PHASES }}
      files={importedFiles((sha256) => (sha256 === SCENE_SHA256 ? `data:text/javascript;base64,${btoa(assetModule(SCENE_PNG))}` : 'data:text/javascript,'))}
    />
  ),
  play: async ({ canvasElement }) => drawnOnTheAsk(canvasElement),
};

export const AskAttachedNotTheBytes: StoryObj = {
  name: 'the operator’s screenshot whose bytes are not its digest: refused on the ask, never drawn',
  render: () => <SessionView session={attached} surface={surface} composer={{ phases: PHASES }} files={() => Promise.resolve({ kind: 'bytes', bytes: SCENE_PNG.slice(0, -1) })} />,
  play: async ({ canvasElement }) => {
    await waitFor(async () => expect(asked(canvasElement)?.querySelector('.ex-file__unshown')?.textContent).toMatch(/^refused: the bytes are not /));
    await expect(asked(canvasElement)?.querySelector('img')).toBeNull();
  },
};

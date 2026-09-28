import type { Preview } from '@storybook/react-vite';

import '../src/theme/tokens.css';
import '../src/theme/themes/index.ts';
import { DEFAULT_PREFS, PREFS, PREF_LABELS } from '../src/ui/prefs.ts';
import type { PrefName, Prefs } from '../src/ui/prefs.ts';
import { Preferred } from '../src/ui/Prefs.tsx';

// Every story renders on the surface's own page and tokens, so a component
// that hard-codes a colour or a face is visible as the odd one out. Each of a
// person's preferences (prefs.ts) is a toolbar switch here, set on the root
// the way the app sets it -- and not remembered, so a story stands alone. A
// story holds a mode of its own (`dark` unless it says), so what it asserts
// does not depend on the machine running it.
const names = Object.keys(PREFS) as PrefName[];

// As a test, a story runs on a desktop wide enough for the trunk, two lanes
// and working memory side by side (addon-vitest sizes the page from the
// story's `viewport`, 1200 by 900 unless it names one); in Storybook it runs
// at whatever the viewer's window is. A story that needs a narrower column
// renders into one (Session: narrow).
const TESTED_ON = { desktop: { name: 'Desktop', styles: { width: '1900px', height: '1100px' } } };

const preview: Preview = {
  parameters: {
    backgrounds: { disable: true },
    layout: 'padded',
    ...(import.meta.env.MODE === 'test' ? { viewport: { options: TESTED_ON, defaultViewport: 'desktop' } } : {}),
  },
  globalTypes: Object.fromEntries(
    names.map((name) => [
      name,
      {
        description: PREF_LABELS[name],
        toolbar: { title: PREF_LABELS[name], items: PREFS[name].map((value) => ({ value, title: `${PREF_LABELS[name]}: ${value}` })), dynamicTitle: true },
      },
    ]),
  ),
  initialGlobals: { ...DEFAULT_PREFS, mode: 'dark' },
  decorators: [
    (Story, context) => {
      const prefs = Object.fromEntries(names.map((name) => [name, context.globals[name] ?? DEFAULT_PREFS[name]])) as unknown as Prefs;
      return (
        <Preferred key={JSON.stringify(prefs)} initial={prefs} remember={false}>
          <Story />
        </Preferred>
      );
    },
  ],
};

export default preview;

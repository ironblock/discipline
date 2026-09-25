import type { Preview } from '@storybook/react-vite';

import '../src/theme/tokens.css';
import { DEFAULT_THEME, THEMES, layersOf } from '../src/theme/themes/index.ts';

// Every story renders on the surface's own page and tokens, so a component
// that hard-codes a colour or a face is visible as the odd one out. The
// toolbar's theme switch sets `data-theme` on the root: every story, every
// theme, the same folded moments.
const preview: Preview = {
  parameters: {
    backgrounds: { disable: true },
    layout: 'padded',
  },
  globalTypes: {
    theme: {
      description: 'Theme to try',
      toolbar: {
        title: 'Theme',
        icon: 'paintbrush',
        items: THEMES.map((t) => ({ value: t.name, title: t.title })),
        dynamicTitle: true,
      },
    },
  },
  initialGlobals: { theme: DEFAULT_THEME },
  decorators: [
    (Story, context) => (
      <div className="ex-root" data-theme={layersOf(String(context.globals['theme'] ?? DEFAULT_THEME))}>
        <Story />
      </div>
    ),
  ],
};

export default preview;

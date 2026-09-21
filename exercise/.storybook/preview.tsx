import type { Preview } from '@storybook/react-vite';

import '../src/theme/tokens.css';

// The catalog renders on the palette `pages/index.html` already publishes:
// dark paper, ink, muted, rule. Every story gets the tokens; a component that
// hard-codes a colour is visible as the odd one out.
const preview: Preview = {
  parameters: {
    backgrounds: { disable: true },
    layout: 'padded',
  },
  decorators: [
    (Story) => (
      <div className="ex-root">
        <Story />
      </div>
    ),
  ],
};

export default preview;

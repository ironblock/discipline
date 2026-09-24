import type { Preview } from '@storybook/react-vite';

import '../src/theme/tokens.css';

// Every story renders on the surface's own page and tokens, so a component
// that hard-codes a colour or a face is visible as the odd one out.
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

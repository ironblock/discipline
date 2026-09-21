import type { StorybookConfig } from '@storybook/react-vite';

// The catalog is organized by the record, not by atomic design. The glob is
// one pattern over `src/` so a story cannot exist in a directory the sidebar
// does not show; the sidebar's four sections come from each story's `title`:
//
//   Fields/     atoms with plain local props, pending fields included
//   Rows/       one story per state the record's doc comments distinguish
//   Sequences/  a fixture `diet check-record` accepts, through the app's loader
//   Pending/    what the record cannot carry yet, drawn dotted, naming its issue
const config: StorybookConfig = {
  framework: '@storybook/react-vite',
  stories: ['../src/**/*.stories.tsx'],
  addons: ['@storybook/addon-docs', '@storybook/addon-vitest'],
  core: {
    disableTelemetry: true,
  },
};

export default config;

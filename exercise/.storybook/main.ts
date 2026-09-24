import type { StorybookConfig } from '@storybook/react-vite';

// Two sections, from each story's `title`:
//
//   Session/   the whole surface at a moment of a driven session
//   Parts/     one component, one story per state it distinguishes
const config: StorybookConfig = {
  framework: '@storybook/react-vite',
  stories: ['../src/**/*.stories.tsx'],
  addons: ['@storybook/addon-docs', '@storybook/addon-vitest'],
  core: {
    disableTelemetry: true,
    disableWhatsNewNotifications: true,
  },
};

export default config;

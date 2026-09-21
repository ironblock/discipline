import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect } from 'storybook/test';

const Smoke = () => <span data-testid="smoke">the harness renders</span>;

const meta = {
  title: 'Fields/Smoke',
  component: Smoke,
} satisfies Meta<typeof Smoke>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Renders: Story = {
  play: async ({ canvas }) => {
    await expect(canvas.getByTestId('smoke')).toHaveTextContent('the harness renders');
  },
};

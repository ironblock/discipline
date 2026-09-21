import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect } from 'storybook/test';

import { pickGroup } from '../fixtures/pick.ts';
import { ToolCallRow, bindToolCall } from './ToolCallRow.tsx';

/**
 * `tool_call` has three optional fields and each has two states the doc
 * comments name: args kept or not; exit kept or not; output absent (not
 * kept) or present-and-empty (printed nothing) or present.
 */
const meta = {
  title: 'Rows/tool_call',
  component: ToolCallRow,
} satisfies Meta<typeof ToolCallRow>;

export default meta;
type Story = StoryObj<typeof meta>;

/** Nothing optional kept: tool and turn only. */
export const NothingKept: Story = {
  name: 'args, exit, output — none kept',
  args: bindToolCall(pickGroup('full-session', 'tool_call')),
  play: async ({ canvas }) => {
    await expect(canvas.getByText('args not kept')).toBeInTheDocument();
    await expect(canvas.getByText('exit not kept')).toBeInTheDocument();
    await expect(canvas.getByText('output not kept')).toBeInTheDocument();
  },
};

/** Exit 0 with output: the size, never the text. */
export const ExitZeroWithOutput: Story = {
  name: 'exit 0 — output as a size',
  args: bindToolCall(pickGroup('archive-rows', 'tool_call', 0)),
  play: async ({ canvas }) => {
    await expect(canvas.getByText('exit 0')).toBeInTheDocument();
    await expect(canvas.queryByText(/pub fn parse/)).not.toBeInTheDocument();
  },
};

/** Exit 0, and the command printed nothing: output present and empty. */
export const PrintedNothing: Story = {
  name: 'exit 0 — printed nothing',
  args: bindToolCall(pickGroup('archive-rows', 'tool_call', 1)),
  play: async ({ canvas }) => {
    await expect(canvas.getByText('printed nothing')).toBeInTheDocument();
  },
};

/** Killed by signal 9. */
export const KilledBySignal: Story = {
  name: 'exit 137 — killed',
  args: bindToolCall(pickGroup('tool-call-exit-statuses', 'tool_call', 0)),
  play: async ({ canvas }) => {
    await expect(canvas.getByText('exit 137')).toBeInTheDocument();
  },
};

/** Never ran: exit -1, empty output. */
export const NotRun: Story = {
  name: 'exit -1 — not run',
  args: bindToolCall(pickGroup('tool-call-exit-statuses', 'tool_call', 1)),
};

/** Command not found, with empty args. */
export const CommandNotFound: Story = {
  name: 'exit 127 — command not found',
  args: bindToolCall(pickGroup('archive-rows', 'tool_call', 2)),
};

/** The drive's shape: `argv` as an array under the `shell` tool. */
export const FromTheDrive: Story = {
  name: 'from the canned drive — argv',
  args: bindToolCall(pickGroup('canned-drive', 'tool_call', 0)),
};

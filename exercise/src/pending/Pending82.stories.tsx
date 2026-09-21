import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect } from 'storybook/test';

import { PendingSequence } from './PendingSequence.tsx';
import type { Expectation } from './PendingSequence.tsx';

import _821TimingOnGenerationEventsProposal from '../../fixtures/pending/82-1-timing-on-generation-events.jsonl?raw';
import _821TimingOnGenerationEventsExpect from '../../fixtures/pending/82-1-timing-on-generation-events.expect.json';
import _822ClockOffsetPerSubstrateProposal from '../../fixtures/pending/82-2-clock-offset-per-substrate.jsonl?raw';
import _822ClockOffsetPerSubstrateExpect from '../../fixtures/pending/82-2-clock-offset-per-substrate.expect.json';
import _823LaneKindChildSessionProposal from '../../fixtures/pending/82-3-lane-kind-child-session.jsonl?raw';
import _823LaneKindChildSessionExpect from '../../fixtures/pending/82-3-lane-kind-child-session.expect.json';
import _824ToolOutputIdempotenceProposal from '../../fixtures/pending/82-4-tool-output-idempotence.jsonl?raw';
import _824ToolOutputIdempotenceExpect from '../../fixtures/pending/82-4-tool-output-idempotence.expect.json';
import _82PrefixColdForkProposal from '../../fixtures/pending/82-prefix-cold-fork.jsonl?raw';
import _82PrefixColdForkExpect from '../../fixtures/pending/82-prefix-cold-fork.expect.json';

/**
 * The burn-down for #82: each story is a `fixtures/pending/*.jsonl` that
 * `diet check-record` refuses today, with the refusal pinned. When one is
 * accepted, `pnpm check:fixtures` goes red naming it, and its story moves
 * to Sequences/.
 */
const meta = {
  title: 'Pending/#82 record v1.1',
  component: PendingSequence,
  parameters: { layout: 'fullscreen' },
} satisfies Meta<typeof PendingSequence>;

export default meta;
type Story = StoryObj<typeof meta>;

/** response.started_at / ended_at / ttft_ms / prefill_tokens_new / prefill_tokens_cached / decode_ms */
export const _821TimingOnGenerationEvents: Story = {
  name: "#82.1 response.started_at / ended_at / ttft_ms / prefill_tokens_new / prefill_tokens_cached / decode_ms",
  args: { name: "82-1-timing-on-generation-events", proposal: _821TimingOnGenerationEventsProposal, expect: _821TimingOnGenerationEventsExpect as Expectation },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("pending #82.1")).toBeInTheDocument();
  },
};

/** substrate.clock_offset_ms */
export const _822ClockOffsetPerSubstrate: Story = {
  name: "#82.2 substrate.clock_offset_ms",
  args: { name: "82-2-clock-offset-per-substrate", proposal: _822ClockOffsetPerSubstrateProposal, expect: _822ClockOffsetPerSubstrateExpect as Expectation },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("pending #82.2")).toBeInTheDocument();
  },
};

/** fork.kind = canonical | fork | child_session */
export const _823LaneKindChildSession: Story = {
  name: "#82.3 fork.kind = canonical | fork | child_session",
  args: { name: "82-3-lane-kind-child-session", proposal: _823LaneKindChildSessionProposal, expect: _823LaneKindChildSessionExpect as Expectation },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("pending #82.3")).toBeInTheDocument();
  },
};

/** tool_call.idempotent */
export const _824ToolOutputIdempotence: Story = {
  name: "#82.4 tool_call.idempotent",
  args: { name: "82-4-tool-output-idempotence", proposal: _824ToolOutputIdempotenceProposal, expect: _824ToolOutputIdempotenceExpect as Expectation },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("pending #82.4")).toBeInTheDocument();
  },
};

/** prefix.cold_fork event */
export const _82PrefixColdFork: Story = {
  name: "#82 prefix.cold_fork event",
  args: { name: "82-prefix-cold-fork", proposal: _82PrefixColdForkProposal, expect: _82PrefixColdForkExpect as Expectation },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("pending #82")).toBeInTheDocument();
  },
};

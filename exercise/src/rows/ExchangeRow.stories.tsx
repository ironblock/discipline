import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect } from 'storybook/test';

import { pickGroup } from '../fixtures/pick.ts';
import { ExchangeRow, bindExchange } from './ExchangeRow.tsx';

/**
 * A request, its retry chain and its response. Every optional field is at
 * least two stories: `request.text` kept or not, `response.text` kept, not
 * kept, or present and empty; a chain answered or still running.
 */
const meta = {
  title: 'Rows/request + response',
  component: ExchangeRow,
} satisfies Meta<typeof ExchangeRow>;

export default meta;
type Story = StoryObj<typeof meta>;

/** A ledger row: neither text kept. Provenance never depended on the payload. */
export const LedgerNoTexts: Story = {
  name: 'ledger — no texts kept',
  args: bindExchange(pickGroup('full-session', 'exchange')),
  play: async ({ canvas }) => {
    await expect(canvas.getAllByText('ledger · text not kept')).toHaveLength(2);
  },
};

/** An archive row: the wire body as a size, the answer as prose. */
export const ArchiveTextsKept: Story = {
  name: 'archive — texts kept',
  args: bindExchange(pickGroup('archive-rows', 'exchange', 0)),
  play: async ({ canvas }) => {
    await expect(canvas.getByText(/wire body/)).toBeInTheDocument();
    await expect(canvas.getByText(/The parser keeps every line/)).toBeInTheDocument();
  },
};

/** `response.text` present and empty: an answer of nothing, typed. */
export const EmptyAnswer: Story = {
  name: 'response.text = "" — a typed outcome',
  args: bindExchange(pickGroup('archive-rows', 'exchange', 1)),
  play: async ({ canvas }) => {
    await expect(canvas.getByText(/empty answer/)).toBeInTheDocument();
    await expect(canvas.getByText('0')).toBeInTheDocument();
  },
};

/** A retry chain: q2 retries q1, and a1 answers q2. One row. */
export const RetryChain: Story = {
  name: 'retry chain — answered',
  args: bindExchange(pickGroup('retry-lineage', 'exchange')),
  play: async ({ canvas }) => {
    await expect(canvas.getByText('retry_of')).toBeInTheDocument();
    await expect(canvas.getByText('to_request')).toBeInTheDocument();
  },
};

/** A request with no response yet: the lane is running, and the drive input derives from this. */
export const LaneRunning: Story = {
  name: 'lane running — no response yet',
  args: bindExchange(pickGroup('lane-running', 'exchange')),
  play: async ({ canvas }) => {
    await expect(canvas.getByText(/lane running/)).toBeInTheDocument();
  },
};

/** A retry with no response yet: still running after a retry. */
export const RetryRunning: Story = {
  name: 'retry chain — still running',
  args: bindExchange(pickGroup('retry-still-running', 'exchange')),
};

/** The interview lane's exchange from the canned drive: the fork's answer, tags as written. */
export const InterviewAnswer: Story = {
  name: 'interview lane — the fork answer',
  args: bindExchange(pickGroup('canned-drive', 'exchange', 1)),
  play: async ({ canvas }) => {
    await expect(canvas.getByText(/DECISION: keep the reconciler/)).toBeInTheDocument();
  },
};

/** The ratify lane's exchange: the ask the seam put, and the answer the drive kept and did not fold. */
export const RatifyAsk: Story = {
  name: 'ratify lane — the seam’s ask',
  args: bindExchange(pickGroup('canned-drive', 'exchange', 4)),
};

/** The kwarg-control lane, before turn one exists: no turn to be contained by. */
export const ControlBeforeTurnOne: Story = {
  name: 'control lane — before turn one',
  args: bindExchange(pickGroup('control-lane-before-turn-one', 'exchange', 0)),
  play: async ({ canvas }) => {
    await expect(canvas.getByText('control')).toBeInTheDocument();
  },
};

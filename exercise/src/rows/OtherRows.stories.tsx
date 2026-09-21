import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect } from 'storybook/test';

import { pickGroup } from '../fixtures/pick.ts';
import { ClaimRow, bindClaim } from './ClaimRow.tsx';
import { RejectedRow, bindRejected } from './RejectedRow.tsx';
import { StartRow, bindStart } from './StartRow.tsx';
import { SummaryRow, bindSummary } from './SummaryRow.tsx';
import { TurnRow, bindTurn } from './TurnRow.tsx';
import { UnknownRow, bindUnknown } from './UnknownRow.tsx';

const meta = {
  title: 'Rows/start · turn · rejected · claim · summary · unknown',
} satisfies Meta;

export default meta;
type Story = StoryObj<typeof meta>;

/** `start`, live, one substrate with weights on disk. */
export const StartLive: Story = {
  name: 'start — live, one substrate',
  render: () => <StartRow {...bindStart(pickGroup('full-session', 'start'))} />,
};

/** `start`, adapted from a foreign log pinned by digest; reasoning undeclared. */
export const StartAdapted: Story = {
  name: 'start — adapted, pinned only, reasoning undeclared',
  render: () => <StartRow {...bindStart(pickGroup('unmapped-row', 'start'))} />,
  play: async ({ canvas }) => {
    await expect(canvas.getByText('undeclared')).toBeInTheDocument();
    await expect(canvas.getByText(/pinned only/)).toBeInTheDocument();
  },
};

/** `start` with two substrates that differ only by chat template digest. */
export const StartTwoTemplates: Story = {
  name: 'start — one weights, two templates',
  render: () => <StartRow {...bindStart(pickGroup('one-weights-two-chat-templates', 'start'))} />,
};

/** `start` with reasoning control declared: capped on one, uncapped on the other. */
export const StartReasoningControl: Story = {
  name: 'start — reasoning control declared',
  render: () => <StartRow {...bindStart(pickGroup('reasoning-control-declared', 'start'))} />,
  play: async ({ canvas }) => {
    await expect(canvas.getByText(/cap 4096 tok/)).toBeInTheDocument();
    await expect(canvas.getByText(/asked medium · uncapped/)).toBeInTheDocument();
  },
};

/** `start` with hosted weights beside local ones. */
export const StartHosted: Story = {
  name: 'start — hosted and local',
  render: () => <StartRow {...bindStart(pickGroup('weights-hosted', 'start'))} />,
};

/** `start` on the canned substrate: no weights, an acts digest. */
export const StartCanned: Story = {
  name: 'start — canned',
  render: () => <StartRow {...bindStart(pickGroup('canned-drive', 'start'))} />,
};

/** `start` whose sampler card lost its exact decimals on the way in: `0.7000` reads as `0.7`. */
export const StartLossyDecimals: Story = {
  name: 'start — exact decimals lost at the JSON boundary',
  render: () => <StartRow {...bindStart(pickGroup('exact-decimals', 'start'))} />,
};

/** `turn`: the section header. */
export const Turn: Story = {
  name: 'turn',
  render: () => <TurnRow {...bindTurn(pickGroup('full-session', 'turn'))} />,
  play: async ({ canvas }) => {
    await expect(canvas.getByText('1,024')).toBeInTheDocument();
  },
};

/** `rejected`: 3 of 30 grounded, on a lane the drive does not name. */
export const Rejected: Story = {
  name: 'rejected — 3 / 30',
  render: () => <RejectedRow {...bindRejected(pickGroup('rejected-lane', 'rejected'))} />,
};

/** `claim`, supported, one artifact. */
export const ClaimSupported: Story = {
  name: 'claim — supported',
  render: () => <ClaimRow {...bindClaim(pickGroup('full-session', 'claim'))} />,
};

/** `claim` corrected: c2 supersedes c1, and c1 stays visible, struck. */
export const ClaimCorrected: Story = {
  name: 'claim — corrected by a later claim',
  render: () => <ClaimRow {...bindClaim(pickGroup('correction-supersedes', 'claim'))} />,
  play: async ({ canvas }) => {
    await expect(canvas.getByText('refuted')).toBeInTheDocument();
    await expect(canvas.getByText('superseded')).toBeInTheDocument();
  },
};

/** `claim`, inconclusive, whose hypothesis carries every escape the value space allows. */
export const ClaimEscapes: Story = {
  name: 'claim — inconclusive, with escapes',
  render: () => <ClaimRow {...bindClaim(pickGroup('string-escapes', 'claim'))} />,
};

/** `summary` of a drive. */
export const SummaryDrive: Story = {
  name: 'summary — drive',
  render: () => <SummaryRow {...bindSummary(pickGroup('canned-drive', 'summary'))} />,
  play: async ({ canvas }) => {
    await expect(canvas.getByText('3 turns')).toBeInTheDocument();
  },
};

/** `summary` of a recompute. */
export const SummaryRecompute: Story = {
  name: 'summary — recompute',
  render: () => <SummaryRow {...bindSummary(pickGroup('recompute-summary', 'summary'))} />,
};

/** `unknown`: a foreign row, verbatim. */
export const Unknown: Story = {
  name: 'unknown — a foreign row kept verbatim',
  render: () => <UnknownRow {...bindUnknown(pickGroup('unmapped-row', 'unknown'))} />,
};

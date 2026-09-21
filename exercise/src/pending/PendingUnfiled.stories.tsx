import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect } from 'storybook/test';

import { PendingSequence } from './PendingSequence.tsx';
import type { Expectation } from './PendingSequence.tsx';

import unfiledEntriesCreatedProposal from '../../fixtures/pending/unfiled-entries-created.jsonl?raw';
import unfiledEntriesCreatedExpect from '../../fixtures/pending/unfiled-entries-created.expect.json';
import unfiledForkToItsRequestProposal from '../../fixtures/pending/unfiled-fork-to-its-request.jsonl?raw';
import unfiledForkToItsRequestExpect from '../../fixtures/pending/unfiled-fork-to-its-request.expect.json';
import unfiledOperatorAskAsAFieldProposal from '../../fixtures/pending/unfiled-operator-ask-as-a-field.jsonl?raw';
import unfiledOperatorAskAsAFieldExpect from '../../fixtures/pending/unfiled-operator-ask-as-a-field.expect.json';
import unfiledPerRequestSlotProposal from '../../fixtures/pending/unfiled-per-request-slot.jsonl?raw';
import unfiledPerRequestSlotExpect from '../../fixtures/pending/unfiled-per-request-slot.expect.json';
import unfiledRequestToItsTurnProposal from '../../fixtures/pending/unfiled-request-to-its-turn.jsonl?raw';
import unfiledRequestToItsTurnExpect from '../../fixtures/pending/unfiled-request-to-its-turn.expect.json';
import unfiledSamplerEchoVerdictProposal from '../../fixtures/pending/unfiled-sampler-echo-verdict.jsonl?raw';
import unfiledSamplerEchoVerdictExpect from '../../fixtures/pending/unfiled-sampler-echo-verdict.expect.json';
import unfiledServingSlotsOnRegimeProposal from '../../fixtures/pending/unfiled-serving-slots-on-regime.jsonl?raw';
import unfiledServingSlotsOnRegimeExpect from '../../fixtures/pending/unfiled-serving-slots-on-regime.expect.json';
import unfiledTangentOpenAndCloseProposal from '../../fixtures/pending/unfiled-tangent-open-and-close.jsonl?raw';
import unfiledTangentOpenAndCloseExpect from '../../fixtures/pending/unfiled-tangent-open-and-close.expect.json';

/**
 * The burn-down for unfiled: each story is a `fixtures/pending/*.jsonl` that
 * `diet check-record` refuses today, with the refusal pinned. When one is
 * accepted, `pnpm check:fixtures` goes red naming it, and its story moves
 * to Sequences/.
 */
const meta = {
  title: 'Pending/unfiled',
  component: PendingSequence,
  parameters: { layout: 'fullscreen' },
} satisfies Meta<typeof PendingSequence>;

export default meta;
type Story = StoryObj<typeof meta>;

/** capture.entries_created */
export const UnfiledEntriesCreated: Story = {
  name: "unfiled/entries-created capture.entries_created",
  args: { name: "unfiled-entries-created", proposal: unfiledEntriesCreatedProposal, expect: unfiledEntriesCreatedExpect as Expectation },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("pending unfiled/entries-created")).toBeInTheDocument();
  },
};

/** request.of_fork */
export const UnfiledForkToItsRequest: Story = {
  name: "unfiled/fork-request-link request.of_fork",
  args: { name: "unfiled-fork-to-its-request", proposal: unfiledForkToItsRequestProposal, expect: unfiledForkToItsRequestExpect as Expectation },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("pending unfiled/fork-request-link")).toBeInTheDocument();
  },
};

/** request.ask (or a read-side projection of request.text) */
export const UnfiledOperatorAskAsAField: Story = {
  name: "unfiled/operator-ask request.ask (or a read-side projection of request.text)",
  args: { name: "unfiled-operator-ask-as-a-field", proposal: unfiledOperatorAskAsAFieldProposal, expect: unfiledOperatorAskAsAFieldExpect as Expectation },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("pending unfiled/operator-ask")).toBeInTheDocument();
  },
};

/** request.slot */
export const UnfiledPerRequestSlot: Story = {
  name: "unfiled/request-slot request.slot",
  args: { name: "unfiled-per-request-slot", proposal: unfiledPerRequestSlotProposal, expect: unfiledPerRequestSlotExpect as Expectation },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("pending unfiled/request-slot")).toBeInTheDocument();
  },
};

/** request.at_turn */
export const UnfiledRequestToItsTurn: Story = {
  name: "unfiled/request-turn-link request.at_turn",
  args: { name: "unfiled-request-to-its-turn", proposal: unfiledRequestToItsTurnProposal, expect: unfiledRequestToItsTurnExpect as Expectation },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("pending unfiled/request-turn-link")).toBeInTheDocument();
  },
};

/** response.echo */
export const UnfiledSamplerEchoVerdict: Story = {
  name: "unfiled/sampler-echo response.echo",
  args: { name: "unfiled-sampler-echo-verdict", proposal: unfiledSamplerEchoVerdictProposal, expect: unfiledSamplerEchoVerdictExpect as Expectation },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("pending unfiled/sampler-echo")).toBeInTheDocument();
  },
};

/** regime.serving.slots */
export const UnfiledServingSlotsOnRegime: Story = {
  name: "unfiled/serving-slots regime.serving.slots",
  args: { name: "unfiled-serving-slots-on-regime", proposal: unfiledServingSlotsOnRegimeProposal, expect: unfiledServingSlotsOnRegimeExpect as Expectation },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("pending unfiled/serving-slots")).toBeInTheDocument();
  },
};

/** tangent event */
export const UnfiledTangentOpenAndClose: Story = {
  name: "unfiled/tangent tangent event",
  args: { name: "unfiled-tangent-open-and-close", proposal: unfiledTangentOpenAndCloseProposal, expect: unfiledTangentOpenAndCloseExpect as Expectation },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("pending unfiled/tangent")).toBeInTheDocument();
  },
};

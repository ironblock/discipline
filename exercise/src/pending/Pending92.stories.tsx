import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect } from 'storybook/test';

import { PendingSequence } from './PendingSequence.tsx';
import type { Expectation } from './PendingSequence.tsx';

import _921SeamReasonAndPrefixHashesProposal from '../../fixtures/pending/92-1-seam-reason-and-prefix-hashes.jsonl?raw';
import _921SeamReasonAndPrefixHashesExpect from '../../fixtures/pending/92-1-seam-reason-and-prefix-hashes.expect.json';
import _922RefusedPhaseProposalProposal from '../../fixtures/pending/92-2-refused-phase-proposal.jsonl?raw';
import _922RefusedPhaseProposalExpect from '../../fixtures/pending/92-2-refused-phase-proposal.expect.json';
import _923RatificationAnswerProposal from '../../fixtures/pending/92-3-ratification-answer.jsonl?raw';
import _923RatificationAnswerExpect from '../../fixtures/pending/92-3-ratification-answer.expect.json';
import _924ForkOutcomeDriveVocabularyProposal from '../../fixtures/pending/92-4-fork-outcome-drive-vocabulary.jsonl?raw';
import _924ForkOutcomeDriveVocabularyExpect from '../../fixtures/pending/92-4-fork-outcome-drive-vocabulary.expect.json';
import _924ForkOutcomeViewerVocabularyProposal from '../../fixtures/pending/92-4-fork-outcome-viewer-vocabulary.jsonl?raw';
import _924ForkOutcomeViewerVocabularyExpect from '../../fixtures/pending/92-4-fork-outcome-viewer-vocabulary.expect.json';
import _925TurnZeroCompositionProposal from '../../fixtures/pending/92-5-turn-zero-composition.jsonl?raw';
import _925TurnZeroCompositionExpect from '../../fixtures/pending/92-5-turn-zero-composition.expect.json';
import _926TypedPatchTargetsProposal from '../../fixtures/pending/92-6-typed-patch-targets.jsonl?raw';
import _926TypedPatchTargetsExpect from '../../fixtures/pending/92-6-typed-patch-targets.expect.json';

/**
 * The burn-down for #92: each story is a `fixtures/pending/*.jsonl` that
 * `diet check-record` refuses today, with the refusal pinned. When one is
 * accepted, `pnpm check:fixtures` goes red naming it, and its story moves
 * to Sequences/.
 */
const meta = {
  title: 'Pending/#92 record v1.2',
  component: PendingSequence,
  parameters: { layout: 'fullscreen' },
} satisfies Meta<typeof PendingSequence>;

export default meta;
type Story = StoryObj<typeof meta>;

/** seam.reason, seam.prefix_hash_before/after, seam.dump_sha256_before/after */
export const _921SeamReasonAndPrefixHashes: Story = {
  name: "#92.1 seam.reason, seam.prefix_hash_before/after, seam.dump_sha256_before/after",
  args: { name: "92-1-seam-reason-and-prefix-hashes", proposal: _921SeamReasonAndPrefixHashesProposal, expect: _921SeamReasonAndPrefixHashesExpect as Expectation },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("pending #92.1")).toBeInTheDocument();
  },
};

/** phase_proposal event */
export const _922RefusedPhaseProposal: Story = {
  name: "#92.2 phase_proposal event",
  args: { name: "92-2-refused-phase-proposal", proposal: _922RefusedPhaseProposalProposal, expect: _922RefusedPhaseProposalExpect as Expectation },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("pending #92.2")).toBeInTheDocument();
  },
};

/** seam.answer */
export const _923RatificationAnswer: Story = {
  name: "#92.3 seam.answer",
  args: { name: "92-3-ratification-answer", proposal: _923RatificationAnswerProposal, expect: _923RatificationAnswerExpect as Expectation },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("pending #92.3")).toBeInTheDocument();
  },
};

/** fork.outcome (complete | truncated | empty | thinking_exhausted) */
export const _924ForkOutcomeDriveVocabulary: Story = {
  name: "#92.4 fork.outcome (complete | truncated | empty | thinking_exhausted)",
  args: { name: "92-4-fork-outcome-drive-vocabulary", proposal: _924ForkOutcomeDriveVocabularyProposal, expect: _924ForkOutcomeDriveVocabularyExpect as Expectation },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("pending #92.4")).toBeInTheDocument();
  },
};

/** fork.outcome (value | decline | mimicry | truncated | unparseable | timeout) */
export const _924ForkOutcomeViewerVocabulary: Story = {
  name: "#92.4 fork.outcome (value | decline | mimicry | truncated | unparseable | timeout)",
  args: { name: "92-4-fork-outcome-viewer-vocabulary", proposal: _924ForkOutcomeViewerVocabularyProposal, expect: _924ForkOutcomeViewerVocabularyExpect as Expectation },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("pending #92.4")).toBeInTheDocument();
  },
};

/** composition event */
export const _925TurnZeroComposition: Story = {
  name: "#92.5 composition event",
  args: { name: "92-5-turn-zero-composition", proposal: _925TurnZeroCompositionProposal, expect: _925TurnZeroCompositionExpect as Expectation },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("pending #92.5")).toBeInTheDocument();
  },
};

/** capture.targets[] */
export const _926TypedPatchTargets: Story = {
  name: "#92.6 capture.targets[]",
  args: { name: "92-6-typed-patch-targets", proposal: _926TypedPatchTargetsProposal, expect: _926TypedPatchTargetsExpect as Expectation },
  play: async ({ canvas }) => {
    await expect(canvas.getByText("pending #92.6")).toBeInTheDocument();
  },
};

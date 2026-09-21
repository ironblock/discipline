import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect } from 'storybook/test';

import { loadFixture } from '../fixtures/pick.ts';
import type { FixtureName } from '../fixtures/pick.ts';
import { Timeline } from './Timeline.tsx';

/**
 * Each story is a fixture `diet check-record` accepts, rendered through the
 * loader the app will use. A sequence earns a story if it exercises a
 * distinct relation pattern, instantiates a named outcome or refusal, or is
 * a moment the thesis names. The title says what it is for.
 */
const meta = {
  title: 'Sequences',
  component: Timeline,
  parameters: { layout: 'fullscreen' },
} satisfies Meta<typeof Timeline>;

export default meta;
type Story = StoryObj<typeof meta>;

const from = (name: FixtureName): Story['args'] => ({ loaded: loadFixture(name) });

/** A plain turn: request, response, tool call, fork, capture, seam, claim, summary -- one of everything. */
export const APlainTurn: Story = {
  name: 'a plain turn (full-session)',
  args: from('full-session'),
  play: async ({ canvas }) => {
    // Ten events, eight rows: the response folded into its request's
    // exchange, the capture into its fork.
    await expect(canvas.getByText('10')).toBeInTheDocument();
    await expect(canvas.getAllByRole('article')).toHaveLength(8);
  },
};

/**
 * The canned drive, end to end: three turns, two forks with their
 * interview exchanges and captures, the ratify exchange and the seam it
 * produced at the operator's declared boundary, and the seam's effect --
 * turn three's request carries the working set the forks captured. The
 * only record in reach that no hand wrote.
 */
export const TheCannedDrive: Story = {
  name: 'the canned drive — forks, captures, a seam (canned-drive)',
  args: from('canned-drive'),
  play: async ({ canvas }) => {
    await expect(canvas.getByText('24')).toBeInTheDocument();
    // Two forks, each with one capture absorbed.
    await expect(canvas.getAllByText('from_fork')).toHaveLength(2);
    // The ratify lane's exchange precedes the seam row.
    await expect(canvas.getByText('ratify')).toBeInTheDocument();
  },
};

/** A retry chain: q2 retries q1, a1 answers q2 -- three events, one row. */
export const ARetryChain: Story = {
  name: 'a retry chain (retry-lineage)',
  args: from('retry-lineage'),
  play: async ({ canvas }) => {
    await expect(canvas.getByText('retry_of')).toBeInTheDocument();
    await expect(canvas.getAllByRole('article')).toHaveLength(3);
  },
};

/** Tool-call exit states: killed by signal 9 (137), and not run (-1) with empty output. */
export const ToolCallExitStates: Story = {
  name: 'tool-call exit states (tool-call-exit-statuses)',
  args: from('tool-call-exit-statuses'),
  play: async ({ canvas }) => {
    await expect(canvas.getByText('exit 137')).toBeInTheDocument();
    await expect(canvas.getByText('not run')).toBeInTheDocument();
    await expect(canvas.getByText('printed nothing')).toBeInTheDocument();
  },
};

/** Archive versus ledger: texts kept, an interview answer that is empty, a command not found. */
export const ArchiveVersusLedger: Story = {
  name: 'archive rows — texts kept, an empty answer, exit 127 (archive-rows)',
  args: from('archive-rows'),
  play: async ({ canvas }) => {
    await expect(canvas.getByText(/empty answer/)).toBeInTheDocument();
    await expect(canvas.getByText('exit 127')).toBeInTheDocument();
  },
};

/** A rejected lane: a fork with no capture, and the rejection with its score. */
export const ARejectedLane: Story = {
  name: 'a rejected lane (rejected-lane)',
  args: from('rejected-lane'),
  play: async ({ canvas }) => {
    await expect(canvas.getByText('rejected whole')).toBeInTheDocument();
    await expect(canvas.getByText('no capture')).toBeInTheDocument();
  },
};

/** A claim correction: c2 supersedes c1; the superseded claim stays visible and struck. */
export const AClaimCorrection: Story = {
  name: 'a claim correction (correction-supersedes)',
  args: from('correction-supersedes'),
  play: async ({ canvas }) => {
    await expect(canvas.getByText('supersedes')).toBeInTheDocument();
    await expect(canvas.getByText('superseded')).toBeInTheDocument();
  },
};

/** Two substrates: the main lane on `big`, the fork on `small` -- the arrangement the repository measures. */
export const TwoSubstrates: Story = {
  name: 'two substrates — a big main lane, a small fork (two-substrates)',
  args: from('two-substrates'),
};

/** Hosted weights beside local ones: the hosted substrate can be observed, never certified. */
export const HostedWeights: Story = {
  name: 'hosted weights (weights-hosted)',
  args: from('weights-hosted'),
};

/** One set of weights served through two chat templates: two substrates, told apart by the template digest alone. */
export const OneWeightsTwoTemplates: Story = {
  name: 'one weights, two chat templates (one-weights-two-chat-templates)',
  args: from('one-weights-two-chat-templates'),
  play: async ({ canvas }) => {
    await expect(canvas.getAllByText('template')).toHaveLength(2);
  },
};

/** Reasoning declared: an effort level and a cap on one substrate, uncapped on the other. */
export const ReasoningDeclared: Story = {
  name: 'reasoning declared — capped and uncapped (reasoning-control-declared)',
  args: from('reasoning-control-declared'),
};

/** Reasoning off. */
export const ReasoningOff: Story = { name: 'reasoning off (reasoning-off)', args: from('reasoning-off') };

/** Reasoning suppressed: requested and not returned, its own state because it is a footgun. */
export const ReasoningSuppressed: Story = { name: 'reasoning suppressed (reasoning-suppressed)', args: from('reasoning-suppressed') };

/** A canned substrate: no weights, acts replayed exactly. */
export const ACannedSubstrate: Story = { name: 'a canned substrate (canned-substrate)', args: from('canned-substrate') };

/** A recompute summary: targets checked and matched, the digests compared. */
export const ARecomputeSummary: Story = {
  name: 'a recompute summary (recompute-summary)',
  args: from('recompute-summary'),
  play: async ({ canvas }) => {
    await expect(canvas.getByText('recompute')).toBeInTheDocument();
  },
};

/** An adapted log carrying a row nothing could map, kept verbatim. */
export const AnUnmappedRow: Story = {
  name: 'an adapted log with an unmapped row (unmapped-row)',
  args: from('unmapped-row'),
  play: async ({ canvas }) => {
    await expect(canvas.getByText('queue-operation')).toBeInTheDocument();
    await expect(canvas.getByText('adapted · claude-code')).toBeInTheDocument();
  },
};

/** The smallest record: a start and nothing else. */
export const Minimal: Story = { name: 'the smallest record (minimal)', args: from('minimal') };

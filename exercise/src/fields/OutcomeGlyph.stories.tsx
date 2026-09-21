import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect } from 'storybook/test';

import { OutcomeGlyph } from './OutcomeGlyph.tsx';
import { Pending } from './Pending.tsx';

const meta = {
  title: 'Fields/OutcomeGlyph',
  component: OutcomeGlyph,
} satisfies Meta<typeof OutcomeGlyph>;

export default meta;
type Story = StoryObj<typeof meta>;

/**
 * The three vocabularies side by side. `#92.4` names one, the drive
 * computes another, the interview grammar types a third; the record carries
 * none. The catalog does not pick -- it shows the disagreement, and the two
 * `fixtures/pending/92-4-fork-outcome-*` fixtures will say which vocabulary
 * lands by which of them turns green.
 */
export const TheDisagreement: Story = {
  args: { vocabulary: 'viewer', outcome: 'value' },
  render: () => (
    <table style={{ borderCollapse: 'collapse' }}>
      <tbody>
        <tr>
          <th style={{ textAlign: 'left', paddingRight: 16 }}>#92.4 (the viewer)</th>
          <td>
            {(['value', 'decline', 'mimicry', 'truncated', 'unparseable', 'timeout'] as const).map((o) => (
              <span key={o} style={{ marginRight: 8 }}>
                <OutcomeGlyph vocabulary="viewer" outcome={o} />
              </span>
            ))}
          </td>
        </tr>
        <tr>
          <th style={{ textAlign: 'left', paddingRight: 16 }}>drive::ForkOutcome</th>
          <td>
            {(['complete', 'truncated', 'empty', 'thinking_exhausted'] as const).map((o) => (
              <span key={o} style={{ marginRight: 8 }}>
                <OutcomeGlyph vocabulary="drive" outcome={o} />
              </span>
            ))}
          </td>
        </tr>
        <tr>
          <th style={{ textAlign: 'left', paddingRight: 16 }}>interview::Completion + Outcome</th>
          <td>
            {(['complete', 'empty', 'truncated', 'value', 'decline', 'unparseable'] as const).map((o) => (
              <span key={o} style={{ marginRight: 8 }}>
                <OutcomeGlyph vocabulary="interview" outcome={o} />
              </span>
            ))}
          </td>
        </tr>
        <tr>
          <th style={{ textAlign: 'left', paddingRight: 16 }}>the record</th>
          <td>
            <Pending issue="#92.4" atom="fork.outcome" why="no fork row carries an outcome in record v0" />
          </td>
        </tr>
      </tbody>
    </table>
  ),
  play: async ({ canvas }) => {
    await expect(canvas.getAllByText('truncated')).toHaveLength(3);
    await expect(canvas.getByText('thinking_exhausted')).toBeInTheDocument();
  },
};

export const Timeout: Story = { args: { vocabulary: 'viewer', outcome: 'timeout' } };
export const ThinkingExhausted: Story = { args: { vocabulary: 'drive', outcome: 'thinking_exhausted' } };

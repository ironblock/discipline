import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect, fn, userEvent, within } from 'storybook/test';

import { Phases } from '../ui/Phases.tsx';
import type { PhaseEvidence, PhaseMove } from '../ui/Phases.tsx';

/**
 * Phases on the page (#789), one story per state: the starting-phase picker before the first ask, the current phase
 * and its moves, the ground rules on asking, the model's proposal ruled with its evidence, and the operator's own
 * move -- proposed, or taken alone where `operator_unilateral` allows. Each control is named with its target (#759).
 */
const MOVES: readonly PhaseMove[] = [
  { to: 'plan', kind: 'suggested' },
  { to: 'quick', kind: 'allowed' },
];
const EVIDENCE: PhaseEvidence = { steps: 14, tokens: 48_200, sinceMeditation: 31_500, manualMeditation: false };
const RULES = 'discuss the user’s intent, consider possible approaches, provide multiple options with trade-offs';

const meta = {
  title: 'Session/Phases',
  component: Phases,
  args: { phase: 'orient', moves: MOVES, groundRules: RULES, evidence: EVIDENCE, unilateral: true, idle: true, onStart: fn(), onRule: fn(), onPropose: fn(), onMove: fn() },
} satisfies Meta<typeof Phases>;

export default meta;
type Story = StoryObj<typeof meta>;

/** Before the first ask: the library to start from, defaulting to the regimen's phase; choosing one starts there. */
export const StartingPicker: Story = {
  name: 'phases · the starting phase, before the first ask',
  args: { start: { library: ['orient', 'plan', 'build', 'test', 'quick'] } },
  play: async ({ canvasElement, args }) => {
    const picker = within(canvasElement).getByRole('combobox', { name: 'starting phase' }) as HTMLSelectElement;
    await expect(picker.value).toBe('orient');
    await expect([...picker.options].map((o) => o.value)).toEqual(['orient', 'plan', 'build', 'test', 'quick']);
    await userEvent.selectOptions(picker, 'build');
    await expect(args.onStart).toHaveBeenCalledWith('build');
  },
};

/** After the first ask: no picker, the phase is the session's. */
export const NoPickerOnceStarted: Story = {
  name: 'phases · no picker once the session has asked',
  play: async ({ canvasElement }) => {
    await expect(within(canvasElement).queryByRole('combobox', { name: 'starting phase' })).toBeNull();
    await expect(within(canvasElement).getByRole('region', { name: 'phase' })).toHaveTextContent('orient');
  },
};

/** The current phase and its moves, each said with its kind. */
export const CurrentAndMoves: Story = {
  name: 'phases · the current phase, its suggested and allowed moves',
  play: async ({ canvasElement }) => {
    const moves = within(canvasElement).getByRole('list', { name: 'moves from orient' });
    await expect(within(moves).getAllByRole('listitem').map((li) => li.textContent)).toEqual(['plan · suggested', 'quick · allowed']);
  },
};

/** The ground rules in force: hidden until asked for, then the text the model was given. */
export const GroundRulesOnAsking: Story = {
  name: 'phases · the ground rules, on asking',
  play: async ({ canvasElement }) => {
    const page = within(canvasElement);
    await expect(page.queryByText(RULES)).toBeNull();
    const ask = page.getByRole('button', { name: 'Ground rules of orient' });
    await expect(ask.getAttribute('aria-expanded')).toBe('false');
    await userEvent.click(ask);
    await expect(ask.getAttribute('aria-expanded')).toBe('true');
    await expect(page.getByText(RULES)).toBeVisible();
  },
};

/** The model proposes: meditate-and-move first, as suggested, then keep, then stay -- each with the evidence. */
export const ModelProposes: Story = {
  name: 'phases · the model proposes, the operator rules with the evidence',
  args: { proposal: { from: 'orient', to: 'plan', reason: 'the options are on the table' } },
  play: async ({ canvasElement, args }) => {
    const page = within(canvasElement);
    const ruling = page.getByRole('region', { name: 'the model proposes orient → plan' });
    await expect(ruling).toHaveTextContent('the options are on the table');
    const evidence = within(ruling).getByRole('definition', { name: 'phase length' });
    await expect(evidence).toHaveTextContent('14 steps · 48.2k tokens');
    await expect(within(ruling).getByRole('definition', { name: 'since the last meditation' })).toHaveTextContent('31.5k tokens');
    const buttons = within(ruling).getAllByRole('button').map((b) => b.getAttribute('aria-label'));
    await expect(buttons).toEqual(['Meditate and move to plan', 'Keep, move to plan', 'Stay in orient']);
    await expect(within(ruling).getByRole('button', { name: 'Meditate and move to plan' }).hasAttribute('data-suggested')).toBe(true);
    await userEvent.click(within(ruling).getByRole('button', { name: 'Keep, move to plan' }));
    await expect(args.onRule).toHaveBeenCalledWith('keep');
    // While a proposal waits, the operator's own move is not offered.
    await expect(page.queryByRole('button', { name: 'Propose plan to the model' })).toBeNull();
  },
};

/** A manual meditation just happened: the evidence says so. */
export const JustMeditated: Story = {
  name: 'phases · the evidence says the operator just meditated',
  args: { proposal: { from: 'orient', to: 'plan' }, evidence: { ...EVIDENCE, sinceMeditation: 900, manualMeditation: true } },
  play: async ({ canvasElement }) => {
    await expect(within(canvasElement).getByRole('definition', { name: 'manual meditation' })).toHaveTextContent('just now');
  },
};

/** The operator moves: with `operator_unilateral` on, move now (meditate-and-move or keep) or propose to the model. */
export const OperatorUnilateral: Story = {
  name: 'phases · the operator may move alone, or propose',
  play: async ({ canvasElement, args }) => {
    const page = within(canvasElement);
    await expect(page.getByRole('definition', { name: 'phase length' })).toHaveTextContent('14 steps · 48.2k tokens');
    await userEvent.click(page.getByRole('button', { name: 'Meditate and move to plan' }));
    await expect(args.onMove).toHaveBeenCalledWith('plan', 'meditation');
    await userEvent.click(page.getByRole('button', { name: 'Keep, move to quick' }));
    await expect(args.onMove).toHaveBeenCalledWith('quick', 'keep');
    await userEvent.click(page.getByRole('button', { name: 'Propose plan to the model' }));
    await expect(args.onPropose).toHaveBeenCalledWith('plan');
  },
};

/** With `operator_unilateral` off, the operator only proposes. */
export const OperatorProposesOnly: Story = {
  name: 'phases · without unilateral, the operator only proposes',
  args: { unilateral: false },
  play: async ({ canvasElement }) => {
    const page = within(canvasElement);
    await expect(page.getByRole('button', { name: 'Propose plan to the model' })).toBeEnabled();
    await expect(page.queryByRole('button', { name: 'Meditate and move to plan' })).toBeNull();
    await expect(page.queryByRole('button', { name: 'Keep, move to plan' })).toBeNull();
    await expect(page.getByRole('definition', { name: 'since the last meditation' })).toHaveTextContent('31.5k tokens');
  },
};

/** While a turn runs, nothing moves: the controls are there, disabled. */
export const DuringATurn: Story = {
  name: 'phases · during a turn, the moves wait',
  args: { idle: false },
  play: async ({ canvasElement }) => {
    await expect(within(canvasElement).getByRole('button', { name: 'Propose plan to the model' })).toBeDisabled();
  },
};

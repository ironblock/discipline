import { createContext, useContext, useState } from 'react';

import type { Ack, Decision, Prompt } from '../drive/transport.ts';
import type { ToolNode } from '../session/fold.ts';
import { SegmentList } from './GateSegments.tsx';
import { heldOf } from './sets.ts';
import './approval.css';

/**
 * The call waiting on the operator, and how to answer it: what the trunk's
 * tool block and the dock under the composer both read. Absent `decide`:
 * nothing can answer (a replay), and nothing waits.
 */
export interface Approving {
  readonly waiting: Prompt | undefined;
  readonly decide?: (call: string, scope: Decision) => Promise<Ack>;
}

export const ApprovalContext = createContext<Approving>({ waiting: undefined });

/** The prompt NODE waits on, if one does: found by its request and id, as its line will be. */
export function usePromptOf(node: ToolNode): Prompt | undefined {
  const { waiting } = useContext(ApprovalContext);
  return waiting && node.call && node.outcome === undefined && waiting.request === node.call.request && waiting.id === node.call.id ? waiting : undefined;
}

/** The four answers, in the order offered: the narrowest scope first, and decline apart (#389). */
export const DECISIONS: readonly { readonly scope: Decision; readonly label: string; readonly title: string }[] = [
  { scope: 'once', label: 'once', title: 'run this exact line, this once' },
  { scope: 'session', label: 'for this session', title: 'run this and any command of its shape until the session ends' },
  { scope: 'workspace', label: 'for this workspace', title: 'run this and any command of its shape in this worktree, from now on' },
  { scope: 'decline', label: 'decline', title: 'do not run it: the model is told it was declined' },
];

/**
 * A command waiting on the operator (#389): what it would run and where, each
 * segment as the gate read it and why it prompted, and the four answers, one
 * click each. Docked at the foot of the page, over the composer, until it is
 * answered: the session does nothing else until then.
 */
export function ApprovalPrompt({ prompt, decide }: { readonly prompt: Prompt; readonly decide: (call: string, scope: Decision) => Promise<Ack> }) {
  const [sent, setSent] = useState<Decision | undefined>(undefined);
  const [refused, setRefused] = useState<string | undefined>(undefined);
  const answer = async (scope: Decision) => {
    setSent(scope);
    setRefused(undefined);
    const ack = await decide(prompt.id, scope);
    if (!ack.ok) {
      setSent(undefined);
      setRefused(ack.refused);
    }
  };
  return (
    <section className="ex-panel ex-approval" role="alertdialog" aria-label="a command waits on you" data-call={prompt.id}>
      <header className="ex-approval__head">
        <span className="ex-approval__title">a command waits on you</span>
        <span className="ex-approval__reason" title={`why the gate held it: ${prompt.reason}`}>
          {heldOf(prompt.reason).label}
        </span>
      </header>
      <div className="ex-approval__cwd" title="the directory it would run in">
        in <code>{prompt.cwd || 'a directory the drive did not say'}</code>
      </div>
      <pre className="ex-approval__command">
        <span className="ex-tool__prompt">$ </span>
        {prompt.command}
      </pre>
      {prompt.segments.length > 0 ? <SegmentList segments={prompt.segments} /> : null}
      <div className="ex-approval__answers">
        {DECISIONS.map((d) => (
          <button
            key={d.scope}
            type="button"
            className="ex-approval__answer"
            data-scope={d.scope}
            title={d.title}
            disabled={sent !== undefined}
            aria-pressed={sent === d.scope}
            onClick={() => void answer(d.scope)}
          >
            {d.label}
          </button>
        ))}
        {refused !== undefined ? (
          <span className="ex-approval__refused" role="status">
            not taken: {refused}
          </span>
        ) : null}
      </div>
    </section>
  );
}

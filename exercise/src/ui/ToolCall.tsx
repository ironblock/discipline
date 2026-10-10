import { useMemo, useState } from 'react';

import type { AssistantNode, Folded, ToolNode } from '../session/fold.ts';
import { Block } from './Block.tsx';
import { bytes, count, lines, took } from './format.ts';
import { Copy } from './Copy.tsx';
import { alarmOf, approvalOf, callOf, callOutcomeOf, callRefusalOf, heldOf } from './sets.ts';
import { usePromptOf } from './Approval.tsx';
import { JudgedSegments } from './GateSegments.tsx';
import { writingOf, writtenApart } from './flow.ts';
import { Flowing } from './Flowing.tsx';
import { FileResults } from './Files.tsx';
import { elapsed, useNow } from './surface.tsx';
import './tool.css';

/** How much of a script, and of what it printed, shows before the block is opened. */
const PEEK = 3;

/**
 * A tool call, one block of the trunk, set as a message is: its header what
 * went in, its footer what came out. What went in is what the model wrote
 * -- an in-turn step, so its tokens written are the call's input: where the
 * drive said where the calls began (`callsFrom`), the calls' share, on the
 * first call of a step; where it did not, the message and its calls were
 * written as one and only the whole is known, so the first call carries the
 * whole, marked as both. Its body reads like a REPL: the command, and what
 * it printed under it, their first lines until opened. What came out is the
 * tool's stats rather than tokens -- lines and bytes and how long, its exit.
 * And what became of it, as the log's `tool_call` says (v3, #297): a call
 * the drive refused says so and why, and printed nothing; one that failed
 * under its policy says that, never a plain failure; what confined it, where
 * the log says. A call the gate held for the operator (#389) waits on them,
 * answered in the dock under the composer; one that ran under a decision says
 * which, why it prompted and how long the operator took (log v4's `approval`,
 * #388), all from its line, so a replay draws it as the session did.
 */
export function ToolBlock({ node, caller, first = false }: { readonly node: Folded<ToolNode>; readonly caller?: Folded<AssistantNode> | undefined; readonly first?: boolean }) {
  const [open, setOpen] = useState(false);
  const since = elapsed(useNow(), node.startedAt);
  const prompt = usePromptOf(node);
  // Arguments that do not read as the object a tool takes are shown as the model wrote them.
  const call = node.arguments === '' || Object.keys(node.args).length > 0 ? callOf(node.tool, node.args) : { label: node.tool, prompt: '', text: `${node.tool}(${node.arguments})`, known: false };
  const outcome = node.outcome !== undefined ? callOutcomeOf(node.outcome) : undefined;
  const refused = node.outcome === 'refused';
  const policyFailed = node.outcome === 'command_failed';
  const script = call.text.split('\n');
  const output = node.output ?? '';
  // What it printed, taken apart once per output, not once per render: every block re-renders on every event
  // of a session, and splitting and encoding a long output each time was most of a replay's script (scripts/perf.mjs).
  const printed = useMemo(() => (output === '' ? [] : output.replace(/\n$/, '').split('\n')), [output]);
  const said = useMemo(() => (output === '' ? 'no output' : `${count(lines(output))} ${lines(output) === 1 ? 'line' : 'lines'} · ${bytes(output)}`), [output]);
  const hidden = Math.max(0, script.length - PEEK) + Math.max(0, printed.length - PEEK);
  const apart = first && caller ? writtenApart(caller) : undefined;
  const whole = first && caller && !apart ? writingOf(caller, 0) : undefined;
  const exit = node.exit !== undefined && {
    value: <span className={node.exit === 0 && !policyFailed ? 'ex-exit ex-exit--ok' : 'ex-exit ex-exit--bad'}>exit {node.exit}</span>,
    title: 'the exit status',
  };
  // What the call came to, where it is more than ran: refused and why, failed under a policy, cancelled.
  const became = outcome &&
    node.outcome !== 'ran' && {
      value: (
        <span className="ex-call-outcome" data-outcome={node.outcome} data-level={outcome.level}>
          {outcome.label}
          {refused && node.refusal !== undefined ? ` · ${callRefusalOf(node.refusal).label}` : ''}
        </span>
      ),
      title: refused ? 'the drive refused the call: it did not run' : policyFailed ? `it ran, and failed under its policy: ${node.policy ?? 'the log names none'}` : 'what became of the call',
    };
  // What it ran under: the operator's decision and why the gate held it, or the pre-seeded set.
  const approval = node.approval && approvalOf(node.approval.scope);
  const approved = node.approval &&
    approval && {
      value: (
        <span className="ex-approval-chip" data-scope={node.approval.scope} data-level={approval.level}>
          {approval.label}
          {node.approval.decided_at !== undefined ? ` · ${took(Math.max(0, node.approval.decided_at - node.startedAt))} to decide` : ''}
        </span>
      ),
      title: node.approval.why !== undefined ? `the gate held it: ${node.approval.why}` : 'what it ran under',
    };
  const confined = node.confinement && {
    value: (
      <span className="ex-confinement">
        {[node.confinement.isolation !== undefined ? `isolation ${node.confinement.isolation}` : undefined, node.confinement.network !== undefined ? `network ${node.confinement.network}` : undefined].filter(Boolean).join(' · ')}
      </span>
    ),
    title: node.confinement.confined ? `what ran: ${node.confinement.confined.join(' ')}` : 'what confined it',
  };
  return (
    <Block
      tone="tool"
      label={call.label}
      {...(apart ? { input: <Flowing flow={apart.calls} title="tokens written for the tool calls" /> } : {})}
      {...(whole
        ? {
            input: (
              <>
                <Flowing flow={whole} title="tokens written, the message above and its calls together: the drive did not say where the calls began" />
                <span className="ex-tool__both" title="written as one: the drive did not say where the calls began">
                  {' · message and call'}
                </span>
              </>
            ),
          }
        : {})}
      live={node.running}
      alarm={prompt ? 'warn' : refused || policyFailed ? alarmOf(outcome!.level) : node.exit !== undefined && node.exit !== 0 ? 'bad' : undefined}
      output={
        prompt ? (
          <span className="ex-elapsed" data-level="slow">
            waiting on you · {took(since.ms)}
          </span>
        ) : node.running ? (
          <span className="ex-elapsed" data-level={since.level}>
            running · {took(since.ms)}
          </span>
        ) : node.writing ? (
          'being written'
        ) : node.waiting ? (
          'waiting for the call before it'
        ) : refused ? (
          'did not run'
        ) : node.outcome === 'cancelled' ? (
          'cut off before it finished'
        ) : (
          `${said} in ${took(node.ms ?? 0)}`
        )
      }
      stats={[became, approved, confined, exit]}
      provenance={node}
      id={node.id}
      actions={
        <>
          <Copy text={call.text} label={call.prompt === '$' ? 'copy command' : 'copy call'} />
          {output !== '' ? <Copy text={output} label="copy output" /> : null}
        </>
      }
    >
      {node.cwd !== undefined || prompt ? <div className="ex-tool__cwd">in {node.cwd ?? prompt?.cwd}</div> : null}
      {node.approval?.why !== undefined ? <div className="ex-tool__held" title={`held: ${node.approval.why}`}>held: {heldOf(node.approval.why).label}</div> : null}
      {/* What the gate read, re-derived from the logged argv: the live prompt's rows, in a replay (#389). */}
      {node.argv && !prompt && (node.approval?.why !== undefined || node.refusal === 'denylist' || node.refusal === 'declined') ? <JudgedSegments argv={node.argv} /> : null}
      <pre className="ex-tool__call">
        {call.prompt ? <span className="ex-tool__prompt">{call.prompt} </span> : null}
        {open ? call.text : script.slice(0, PEEK).join('\n')}
        {!open && script.length > PEEK ? <span className="ex-tool__clip">{'\n'}…</span> : null}
      </pre>
      {printed.length > 0 ? <pre className="ex-tool__output">{open ? output : printed.slice(0, PEEK).join('\n')}</pre> : null}
      {node.stderr ? <pre className="ex-tool__output ex-tool__stderr">{node.stderr}</pre> : null}
      {node.files ? <FileResults files={node.files} /> : null}
      {hidden > 0 ? (
        <button type="button" className="ex-more" aria-expanded={open} onClick={() => setOpen(!open)}>
          {open ? 'less' : `${count(hidden)} more ${hidden === 1 ? 'line' : 'lines'}`}
        </button>
      ) : null}
    </Block>
  );
}

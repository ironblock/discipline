import { bind, bindOptional } from '../record/bound.ts';
import type { Bound } from '../record/bound.ts';
import type { Group } from '../record/group.ts';
import type { Value } from '../record/types.ts';
import { ByteSize } from '../fields/ByteSize.tsx';
import { ExitStatus } from '../fields/ExitStatus.tsx';
import { IdChip } from '../fields/IdChip.tsx';
import { Pending } from '../fields/Pending.tsx';
import { Row, Spacer } from './Row.tsx';

export interface ToolCallRowProps {
  readonly id: Bound<string>;
  readonly atTurn: Bound<number>;
  readonly tool: Bound<string>;
  readonly args?: Bound<{ readonly [name: string]: Value }>;
  readonly exit?: Bound<number>;
  /** Absent: not kept. Present and empty: the command printed nothing. */
  readonly output?: Bound<string>;
}

/**
 * A tool call: tool, args, output SIZE, exit. The output is never inlined --
 * it is the thing whose cost the program exists to avoid paying twice. The
 * three optional fields each have two states the row must tell apart.
 */
export function ToolCallRow({ id, atTurn, tool, args, exit, output }: ToolCallRowProps) {
  return (
    <Row
      kind="tool_call"
      containment={{ by: 'link', turn: atTurn.value }}
      head={
        <>
          <IdChip id={id.value} kind="tool_call" />
          <span style={{ fontFamily: 'var(--font-tool)' }}>{tool.value}</span>
          {args ? (
            Object.entries(args.value).map(([k, v]) => (
              <span className="ex-kv" key={k}>
                <span className="ex-kv__k">{k}</span>
                <span>{typeof v === 'string' ? v : JSON.stringify(v)}</span>
              </span>
            ))
          ) : (
            <span className="ex-label">args not kept</span>
          )}
          <Pending issue="#82.4" atom="tool_call.idempotent" why="idempotent or state-dependent decides whether the output can be evicted and replayed" />
          <Spacer />
          <OutputSize output={output} />
          {exit ? <ExitStatus exit={exit.value} /> : <span className="ex-label">exit not kept</span>}
        </>
      }
    />
  );
}

function OutputSize({ output }: { readonly output: Bound<string> | undefined }) {
  if (!output) return <span className="ex-label">output not kept</span>;
  if (output.value === '') return <span className="ex-label">printed nothing</span>;
  return (
    <span className="ex-label">
      output <ByteSize bytes={new TextEncoder().encode(output.value).length} />
    </span>
  );
}

export function bindToolCall(group: Extract<Group, { kind: 'tool_call' }>): ToolCallRowProps {
  const args = bindOptional(group.head, 'args');
  const exit = bindOptional(group.head, 'exit');
  const output = bindOptional(group.head, 'output');
  return {
    id: bind(group.head, 'id'),
    atTurn: bind(group.head, 'at_turn'),
    tool: bind(group.head, 'tool'),
    ...(args ? { args } : {}),
    ...(exit ? { exit } : {}),
    ...(output ? { output } : {}),
  };
}

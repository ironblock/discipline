import type { Flow } from './flow.ts';
import { flowText } from './flow.ts';

/** A flow as a block says it; while it runs, coloured by how worrying its time has become. */
export function Flowing({ flow, level, title }: { readonly flow: Flow; readonly level?: 'ok' | 'slow' | 'stalled'; readonly title?: string }) {
  return (
    <span className={flow.running ? 'ex-elapsed' : undefined} data-level={flow.running ? level : undefined} title={title}>
      {flowText(flow)}
    </span>
  );
}

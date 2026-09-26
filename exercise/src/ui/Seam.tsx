import type { Folded, SeamNode } from '../session/fold.ts';
import { ms, tokens } from './format.ts';
import { seamReasonOf } from './sets.ts';
import './seam.css';

/**
 * The one deliberate prefill event, drawn across the trunk: the phase it
 * moved between, what the prefix was and what it became, and the pre-warm
 * that made the next ask cheap.
 */
export function Seam({ node }: { readonly node: Folded<SeamNode> }) {
  return (
    <div className="ex-seam" role="separator" data-from={node.from.join(' ')} data-needs={node.needs.join(' ')} data-id={node.id}>
      <span className="ex-seam__rule" aria-hidden="true" />
      <div className="ex-seam__label">
        <span className="ex-seam__kind">refill</span>
        {node.phase ? (
          <span>
            {node.phase.from} → <strong>{node.phase.to}</strong>
          </span>
        ) : (
          <span title="the record does not say which phases this seam moved between">phase not recorded</span>
        )}
        <span title="the trunk's prefix before and after">
          {node.prefixBefore !== undefined ? `${tokens(node.prefixBefore)} → ` : ''}
          <strong>{tokens(node.prefixAfter)}</strong> tok
        </span>
        <span title="prefix hash before and after">
          {node.hashBefore} → {node.hashAfter}
        </span>
        <span title="the pre-warm: the new prefix sent once so the next ask finds it cached">warm {ms(node.warm.prompt_ms)}</span>
        <span className="ex-seam__reason" data-level={seamReasonOf(node.reason).level}>
          {seamReasonOf(node.reason).label}
        </span>
      </div>
      <span className="ex-seam__rule" aria-hidden="true" />
    </div>
  );
}

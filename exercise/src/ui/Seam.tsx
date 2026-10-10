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
    <div className="ex-seam" role="separator" data-reason={node.reason} data-from={node.from.join(' ')} data-needs={node.needs.join(' ')} id={node.id} data-id={node.id}>
      <span className="ex-seam__rule" aria-hidden="true" />
      <div className="ex-seam__label">
        <span className="ex-seam__kind">refill</span>
        {/* An automatic seam (#633): fired by the window, at the size that fired it -- not declared by anyone. */}
        {/* The operator's changes the refill carried, and the flags it left unaddressed (#150). */}
        {node.operatorChanges || node.unaddressed ? (
          <span className="ex-seam__operator" title="what you changed in working memory since the last seam, and the flags this seam did not address">
            {[
              ...(node.operatorChanges ? [`operator: ${node.operatorChanges.map((c) => `${c.kind} ${c.entry}`).join(', ')}`] : []),
              ...(node.unaddressed ? [`unaddressed: ${node.unaddressed.join(', ')}`] : []),
            ].join(' · ')}
          </span>
        ) : null}
        {/* A tail that kept no turn (#693), in the maintainer's words: the newest turn outgrew the tail's budget. */}
        {node.zeroTurns ? (
          <span className="ex-seam__zero" title="the compaction tail keeps whole turns, newest first; the newest was larger than its budget, so none was kept">
            {`Zero turns retained: last turn was ${node.zeroTurns.newest} against budget ${node.zeroTurns.budget}`}
          </span>
        ) : null}
        {/* Refilled over the operator's unaddressed changes (#150's gate): their call, not the reconcile's. */}
        {node.refilledAnyway ? (
          <span className="ex-seam__anyway" title="the reconcile left your changes unaddressed, and you refilled anyway">
            refilled anyway
          </span>
        ) : null}
        {node.size ? (
          <span className="ex-seam__size" title="an automatic seam: the prompt neared the window, so the trunk was refilled before the next request">
            automatic · {tokens(node.size.promptTokens)} of a {tokens(node.size.window)} window
          </span>
        ) : null}
        {node.phase ? (
          <span>
            {node.phase.from} → <strong>{node.phase.to}</strong>
          </span>
        ) : (
          <span title="the record does not say which phases this seam moved between">phase not recorded</span>
        )}
        {node.prefixAfter !== undefined ? (
          <span title="the trunk's prefix before and after">
            {node.prefixBefore !== undefined ? `${tokens(node.prefixBefore)} → ` : ''}
            <strong>{tokens(node.prefixAfter)}</strong> tok
          </span>
        ) : null}
        <span title={`prefix hash before and after: ${node.hashBefore} → ${node.hashAfter}`}>
          {node.hashBefore.slice(0, 8)} → {node.hashAfter.slice(0, 8)}
        </span>
        {node.carried ? (
          <span title="what the refill carried: working-memory entries, and turns of the old trunk">
            carried {node.carried.entries} {node.carried.entries === 1 ? 'entry' : 'entries'}, {node.carried.turns}{' '}
            {node.carried.turns === 1 ? 'turn' : 'turns'}
          </span>
        ) : null}
        {node.warm ? (
          <span title="the pre-warm: the new prefix sent once so the next ask finds it cached">warm {ms(node.warm.prompt_ms)}</span>
        ) : (
          <span title="the record has no pre-warm for this seam">no pre-warm recorded</span>
        )}
        <span className="ex-seam__reason" data-level={seamReasonOf(node.reason).level}>
          {seamReasonOf(node.reason).label}
        </span>
      </div>
      <span className="ex-seam__rule" aria-hidden="true" />
      {/* The background commands still running, as the refill listed them after its summary (#614). */}
      {node.background && node.background.length > 0 ? (
        <ul className="ex-seam__background" aria-label="background commands still running at this seam">
          {node.background.map((job) => (
            <li key={job.id} className="ex-seam__job" data-job={job.id}>
              <span className="ex-seam__job-id">{job.id}</span>
              <code className="ex-seam__job-command">{job.command}</code>
              <span className="ex-seam__job-since">{job.since}</span>
              <span className="ex-seam__job-output" title="the job's output file">{job.output}</span>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}

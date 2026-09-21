import { bindAt } from '../record/bound.ts';
import type { Bound } from '../record/bound.ts';
import type { Group } from '../record/group.ts';
import type { Source, Substrate } from '../record/types.ts';
import { ReasoningBadge } from '../fields/Badge.tsx';
import { Digest } from '../fields/Digest.tsx';
import { Pending } from '../fields/Pending.tsx';
import { SubstrateBadge } from '../fields/SubstrateBadge.tsx';
import { Row, Spacer } from './Row.tsx';

export interface StartRowProps {
  readonly arm: Bound<string>;
  readonly dogmaVersion: Bound<number>;
  readonly source: Bound<Source>;
  readonly substrates: Bound<readonly Substrate[]>;
}

/**
 * The session begins: the regime, declared once. Every substrate gets a
 * card, because identity is the weights, typed -- and two cards that differ
 * only in `chat_template_sha256` are two substrates, which is the point of
 * the `one-weights-two-chat-templates` fixture.
 *
 * Missing from the regime and drawn dotted: `serving.slots` (ruled on #82,
 * never landed) and `clock_offset_ms` per substrate (#82.2).
 */
export function StartRow({ arm, dogmaVersion, source, substrates }: StartRowProps) {
  const src = source.value;
  return (
    <Row
      kind="start"
      containment={{ by: 'none' }}
      head={
        <>
          <span className="ex-kind">start</span>
          <span className="ex-chip">arm {arm.value}</span>
          <span className="ex-chip ex-muted">dogma v{dogmaVersion.value}</span>
          {src.kind === 'live' ? (
            <span className="ex-chip">live</span>
          ) : (
            <>
              <span className="ex-chip ex-outcome ex-badge--amber">adapted · {src.adapter}</span>
              <span className="ex-label">
                source <Digest sha256={src.source_digest} /> · {src.source_available === 'committed' ? 'committed' : 'pinned only'}
              </span>
            </>
          )}
          <Pending issue="unfiled/serving-slots" atom="regime.serving.slots" why="ruled on #82 as serving.slots and never landed; the slot mode has nothing to draw" />
          <Spacer />
          <span className="ex-label">
            {substrates.value.length} substrate{substrates.value.length === 1 ? '' : 's'}
          </span>
        </>
      }
      body={
        <div style={{ display: 'grid', gap: '0.5rem', gridTemplateColumns: 'repeat(auto-fit, minmax(22rem, 1fr))' }}>
          {substrates.value.map((s) => (
            <SubstrateCard key={s.id} substrate={s} />
          ))}
        </div>
      }
    />
  );
}

function SubstrateCard({ substrate: s }: { readonly substrate: Substrate }) {
  return (
    <div className="ex-substrate-card" data-substrate={s.id}>
      <span className="ex-substrate-card__k">id</span>
      <span>
        <SubstrateBadge id={s.id} weights={s.weights.kind} />
      </span>
      <span className="ex-substrate-card__k">engine</span>
      <span className="ex-substrate-card__v">
        {s.engine.name} · {s.engine.version_or_digest}
      </span>
      <span className="ex-substrate-card__k">weights</span>
      <span className="ex-substrate-card__v">
        {s.weights.kind === 'digest' ? <Digest sha256={s.weights.sha256} /> : null}
        {s.weights.kind === 'hosted' ? `${s.weights.provider} · ${s.weights.model_id} · ${s.weights.version_or_date_observed}` : null}
        {s.weights.kind === 'canned' ? (
          <>
            acts <Digest sha256={s.weights.acts_sha256} />
          </>
        ) : null}
      </span>
      <span className="ex-substrate-card__k">hardware</span>
      <span className="ex-substrate-card__v">
        <Digest sha256={s.hardware_fingerprint} />
      </span>
      <span className="ex-substrate-card__k">reasoning</span>
      <span>
        <ReasoningBadge reasoning={s.reasoning} />
        {s.reasoning_control ? (
          <span className="ex-label" style={{ marginLeft: '0.5rem' }}>
            asked {s.reasoning_control.effort} ·{' '}
            {s.reasoning_control.budget_tokens.kind === 'tokens' ? `cap ${s.reasoning_control.budget_tokens.tokens} tok` : 'uncapped'}
          </span>
        ) : null}
      </span>
      {s.chat_template_sha256 ? (
        <>
          <span className="ex-substrate-card__k">template</span>
          <span className="ex-substrate-card__v">
            <Digest sha256={s.chat_template_sha256} />
          </span>
        </>
      ) : null}
      <span className="ex-substrate-card__k">sampler</span>
      <span className="ex-substrate-card__v">
        {Object.entries(s.sampler_card).map(([k, v]) => (
          <span className="ex-kv" key={k} style={{ marginRight: '0.6rem' }}>
            <span className="ex-kv__k">{k}</span>
            <span>{typeof v === 'string' ? v : JSON.stringify(v)}</span>
          </span>
        ))}
      </span>
      <span className="ex-substrate-card__k">clock</span>
      <span>
        <Pending issue="#82.2" atom="substrate.clock_offset_ms" why="without it, cross-substrate overlap is unverifiable" />
      </span>
    </div>
  );
}

export function bindStart(group: Extract<Group, { kind: 'start' }>): StartRowProps {
  const { head } = group;
  const { regime, source } = head.event;
  return {
    arm: bindAt(head, 'regime.arm', regime.arm),
    dogmaVersion: bindAt(head, 'regime.dogma_version', regime.dogma_version),
    source: bindAt(head, 'source', source),
    substrates: bindAt(head, 'regime.substrates', regime.substrates),
  };
}

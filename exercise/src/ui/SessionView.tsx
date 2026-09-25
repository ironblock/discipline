import { useCallback, useLayoutEffect, useRef, useState } from 'react';

import type { BranchNode, Folded, Session, TrunkNode } from '../session/fold.ts';
import { Branch } from './Branch.tsx';
import { Composer } from './Composer.tsx';
import type { ComposerProps } from './Composer.tsx';
import { Memory, isUnseen } from './Memory.tsx';
import { AssistantMessage, SystemMessage, UserMessage } from './Message.tsx';
import { Seam } from './Seam.tsx';
import { SessionHeader } from './SessionHeader.tsx';
import { SurfaceContext } from './surface.tsx';
import type { Surface } from './surface.tsx';
import { ToolCall } from './ToolCall.tsx';
import './session.css';

export interface SessionViewProps {
  readonly session: Session;
  readonly surface: Surface;
  readonly onSurface?: (next: Surface) => void;
  readonly composer: Omit<ComposerProps, 'state' | 'phase'>;
  /** Keep the newest content in view while it arrives, unless the person scrolled away. */
  readonly follow?: boolean;
}

/** Vertical space between two branches stacked in one slot. */
const STACK_GAP = 14;
/** Where on a block the connector attaches: the middle of a thin bar. */
const ATTACH = 15;

interface Placement {
  readonly top: number;
  readonly anchor: number;
}

/**
 * The surface: the trunk as a conversation on the left and, behind the
 * curtain, one column per other server slot. A branch sits level with the
 * trunk node it came from, in the column of the slot that served it; when
 * an earlier branch in the same slot is still in the way it stacks below
 * and its connector bends to reach it. The trunk never moves for a branch.
 */
export function SessionView({ session, surface, onSurface, composer, follow = false }: SessionViewProps) {
  const lanes = surface.curtain ? laneSlots(session) : [];
  const stage = useRef<HTMLDivElement>(null);
  const end = useRef<HTMLDivElement>(null);
  const anchors = useRef(new Map<string, HTMLElement>());
  const cells = useRef(new Map<string, HTMLElement>());
  const [placed, setPlaced] = useState<ReadonlyMap<string, Placement>>(new Map());
  const [height, setHeight] = useState(0);
  const seams = useRef(new Map<number, HTMLElement>());
  const [seamPad, setSeamPad] = useState<ReadonlyMap<number, number>>(new Map());
  // What the person has acknowledged in working memory: a log position, local to this view.
  const [seenThrough, setSeenThrough] = useState(-1);
  const lastEra = session.eras.length - 1;

  const trunkOrder = session.eras.flatMap((era) => era.nodes.map((n) => n.id));
  const eraOf = new Map(session.eras.flatMap((era) => era.nodes.map((n) => [n.id, era.index] as const)));
  const laneBranches = (slot: number) =>
    trunkOrder.flatMap((id) => (session.branches.get(id) ?? []).filter((b) => b.slot === slot));

  const layout = useCallback(() => {
    const root = stage.current;
    if (!root) return;
    const base = root.getBoundingClientRect().top;
    const next = new Map<string, Placement>();
    // The lowest branch bottom per era, for the seam that closes it.
    const eraBottom = new Map<number, number>();
    let bottom = 0;
    for (const slot of lanes) {
      let floor = 0;
      for (const branch of laneBranches(slot)) {
        const anchorEl = anchors.current.get(branch.at);
        const cellEl = cells.current.get(branch.id);
        if (!anchorEl || !cellEl) continue;
        const anchor = anchorEl.getBoundingClientRect().top - base;
        const top = Math.max(anchor, floor);
        next.set(branch.id, { top, anchor });
        floor = top + cellEl.offsetHeight + STACK_GAP;
        const era = eraOf.get(branch.at) ?? 0;
        eraBottom.set(era, Math.max(eraBottom.get(era) ?? 0, top + cellEl.offsetHeight));
      }
      bottom = Math.max(bottom, floor);
    }
    // A seam is a barrier: the refill happens after every side call before it
    // has finished, so it is drawn below all of them, and only it moves the trunk.
    const pads = new Map<number, number>();
    for (const [era, el] of seams.current) {
      // The wrapper's own top does not move with its padding: the pad is inside it.
      const natural = el.getBoundingClientRect().top - base;
      let before = 0;
      for (const [e, b] of eraBottom) if (e < era) before = Math.max(before, b);
      const pad = Math.max(0, Math.ceil(before + STACK_GAP - natural));
      if (pad > 0) pads.set(era, pad);
    }
    setPlaced((prev) => (samePlacements(prev, next) ? prev : next));
    setHeight((prev) => (prev === bottom ? prev : bottom));
    setSeamPad((prev) => (sameNumbers(prev, pads) ? prev : pads));
    // `lanes` and `laneBranches` are derived from `session` and `surface`.
  }, [session, surface.curtain]);

  useLayoutEffect(() => {
    layout();
    const root = stage.current;
    if (!root) return;
    const observer = new ResizeObserver(() => layout());
    observer.observe(root);
    for (const el of cells.current.values()) observer.observe(el);
    return () => observer.disconnect();
  }, [layout]);

  useLayoutEffect(() => {
    if (!follow) return;
    const nearBottom = window.innerHeight + window.scrollY >= document.body.scrollHeight - 240;
    if (nearBottom) end.current?.scrollIntoView({ block: 'end' });
  }, [follow, session.events]);

  const anchorRef = (id: string) => (el: HTMLElement | null) => {
    if (el) anchors.current.set(id, el);
    else anchors.current.delete(id);
  };
  const seamRef = (era: number) => (el: HTMLElement | null) => {
    if (el) seams.current.set(era, el);
    else seams.current.delete(era);
  };
  const cellRef = (id: string) => (el: HTMLElement | null) => {
    if (el) cells.current.set(id, el);
    else cells.current.delete(id);
  };

  return (
    <SurfaceContext.Provider value={surface}>
      <div
        className={`ex-session${surface.gaps ? ' ex-gaps' : ''}${surface.curtain ? ' ex-session--curtain' : ''}`}
        style={{ ['--lanes' as string]: lanes.length }}
        data-state={session.state}
        data-lanes-busy={session.occupancy.some((holder, slot) => holder !== undefined && slot !== session.trunkSlot) ? '' : undefined}
        data-fresh={session.memory.some((m) => isUnseen(m, seenThrough)) ? '' : undefined}
      >
        <div className="ex-session__header">
          <SessionHeader session={session} surface={surface} {...(onSurface ? { onSurface } : {})} />
        </div>
        <div className="ex-session__main">
          <div className="ex-session__columns">
            {lanes.length > 0 ? (
              <div className="ex-lanehead-row" aria-hidden="true">
                <div className="ex-lanehead">slot {session.trunkSlot} · trunk</div>
                {lanes.map((slot) => (
                  <div className="ex-lanehead" key={slot} data-busy={session.occupancy[slot] === undefined ? undefined : ''}>
                    slot {slot} · {session.occupancy[slot] ?? 'idle'}
                  </div>
                ))}
              </div>
            ) : null}
            <div className="ex-stage" ref={stage}>
              <div className="ex-trunk">
                {session.eras.map((era) => (
                  <section className="ex-era" key={era.index} data-era={era.index}>
                    {era.seam ? (
                      <div
                        className="ex-era__seam"
                        ref={seamRef(era.index)}
                        style={surface.curtain ? { paddingTop: seamPad.get(era.index) ?? 0 } : undefined}
                      >
                        <Seam node={era.seam} />
                      </div>
                    ) : null}
                    <SystemMessage node={era.system} />
                    {era.index === 0 && era.nodes.length === 0 && session.state === 'awaiting' ? (
                      <p className="ex-hint">
                        Your turn. Ask for something, and the answer lands here. Behind the curtain, side calls run in the
                        slots to the right while you read, and what they learn lands in working memory.
                      </p>
                    ) : null}
                    {era.nodes.map((node) => {
                      const branches = session.branches.get(node.id) ?? [];
                      return (
                        <div className="ex-trunk__node" key={node.id} ref={anchorRef(node.id)}>
                          <TrunkBlock node={node} />
                          {!surface.curtain && branches.length > 0 ? (
                            <button
                              type="button"
                              className="ex-peek"
                              title={`${branches.length} side call${branches.length === 1 ? '' : 's'} off this message`}
                              onClick={() => onSurface?.({ ...surface, curtain: true })}
                            >
                              ↳ {branches.length}
                            </button>
                          ) : null}
                        </div>
                      );
                    })}
                  </section>
                ))}
                <div ref={end} className="ex-session__end" />
              </div>
              {lanes.map((slot) => (
                <div className="ex-lane" key={slot} data-slot={slot} style={{ minHeight: height }}>
                  {laneBranches(slot).map((branch) => (
                    <BranchCell
                      key={branch.id}
                      branch={branch}
                      placement={placed.get(branch.id)}
                      cellRef={cellRef(branch.id)}
                      evicted={(eraOf.get(branch.at) ?? lastEra) < lastEra}
                    />
                  ))}
                </div>
              ))}
            </div>
            {/* The composer sits in the trunk's own grid column, so it lines up with the trunk at any width. */}
            <div className="ex-session__composer">
              <Composer key={session.phase} state={session.state} phase={session.phase} {...composer} />
            </div>
          </div>
          {surface.curtain ? (
            <aside className="ex-session__memory">
              <Memory entries={session.memory} seenThrough={seenThrough} onSeen={() => setSeenThrough(session.events - 1)} />
            </aside>
          ) : null}
        </div>
      </div>
    </SurfaceContext.Provider>
  );
}

function TrunkBlock({ node }: { readonly node: TrunkNode }) {
  switch (node.kind) {
    case 'user':
      return <UserMessage node={node} />;
    case 'assistant':
      return <AssistantMessage node={node} />;
    case 'tool':
      return <ToolCall node={node} />;
  }
}

function BranchCell({
  branch,
  placement,
  cellRef,
  evicted,
}: {
  readonly branch: Folded<BranchNode>;
  readonly placement: Placement | undefined;
  readonly cellRef: (el: HTMLElement | null) => void;
  /** Its trunk node was evicted at a later seam. */
  readonly evicted: boolean;
}) {
  // Until the first layout pass the cell is measured in place, invisibly.
  const drop = placement ? placement.top - placement.anchor : 0;
  return (
    <div
      className="ex-branchcell"
      ref={cellRef}
      style={placement ? { top: placement.top } : { top: 0, visibility: 'hidden' }}
      data-branch={branch.id}
      data-evicted={evicted ? '' : undefined}
    >
      <span className="ex-connector" aria-hidden="true" style={{ top: ATTACH - drop, height: drop }} data-bent={drop > 0 ? '' : undefined}>
        <span className="ex-connector__tip" />
      </span>
      <Branch node={branch} />
    </div>
  );
}

function sameNumbers(a: ReadonlyMap<number, number>, b: ReadonlyMap<number, number>): boolean {
  if (a.size !== b.size) return false;
  for (const [k, v] of b) if (Math.abs((a.get(k) ?? -1) - v) > 0.5) return false;
  return true;
}

function samePlacements(a: ReadonlyMap<string, Placement>, b: ReadonlyMap<string, Placement>): boolean {
  if (a.size !== b.size) return false;
  for (const [id, p] of b) {
    const q = a.get(id);
    if (!q || Math.abs(q.top - p.top) > 0.5 || Math.abs(q.anchor - p.anchor) > 0.5) return false;
  }
  return true;
}

/** Every slot but the trunk's, in order. */
function laneSlots(session: Session): number[] {
  return Array.from({ length: session.slots }, (_, i) => i).filter((i) => i !== session.trunkSlot);
}

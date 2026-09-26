import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';

import type { Link } from '../drive/transport.ts';
import type { BranchNode, Folded, Session, TrunkNode } from '../session/fold.ts';
import { Branch, BranchBar } from './Branch.tsx';
import { Cable } from './Cable.tsx';
import { ENTER, place } from './placement.ts';
import type { Placed, Side, Span } from './placement.ts';
import { Composer } from './Composer.tsx';
import type { ComposerProps } from './Composer.tsx';
import { Memory, isUnseen } from './Memory.tsx';
import { Minimap } from './Minimap.tsx';
import { AssistantMessage, SystemMessage, TurnEnd, UserMessage } from './Message.tsx';
import { Seam } from './Seam.tsx';
import { SessionHeader } from './SessionHeader.tsx';
import { laneStyle } from './sets.ts';
import { ClockContext, SurfaceContext } from './surface.tsx';
import type { Surface } from './surface.tsx';
import { ToolCall } from './ToolCall.tsx';
import './session.css';

export interface SessionViewProps {
  readonly session: Session;
  /** The connection to the drive. Absent: live. */
  readonly link?: Link;
  readonly surface: Surface;
  readonly onSurface?: (next: Surface) => void;
  readonly composer: Omit<ComposerProps, 'state' | 'phase'>;
  /** Keep the newest content in view while it arrives, unless the person scrolled away. */
  readonly follow?: boolean;
}

/** Vertical space between two branches stacked in one slot: whole, and condensed to bars. */
const STACK_GAP = 14;
const STACK_GAP_CONDENSED = 4;
/** How near the bottom still counts as at it, for following. */
const LOCK_SLACK = 48;
interface Placement extends Placed {
  /** From the trunk's right edge to the side call's left: what the cable spans. */
  readonly reach: number;
}

/**
 * The surface: the trunk as a conversation on the left and, behind the
 * curtain, one column per other server slot. A branch sits level with the
 * trunk node it came from, in the column of the slot that served it; when
 * an earlier branch in the same slot is still in the way it stacks below
 * and its cable bends to reach it. The trunk never moves for a branch.
 */
export function SessionView({ session, link = 'live', surface, onSurface, composer, follow = false }: SessionViewProps) {
  const lanes = surface.curtain ? laneSlots(session) : [];
  const condensed = surface.curtain && surface.condensed === true;
  const gap = condensed ? STACK_GAP_CONDENSED : STACK_GAP;
  // A bar pressed while condensed: that side call, opened, once the curtain is.
  const [revealed, setRevealed] = useState<string>();
  const scrolledTo = useRef<string>(undefined);
  const stage = useRef<HTMLDivElement>(null);
  const anchors = useRef(new Map<string, HTMLElement>());
  const cells = useRef(new Map<string, HTMLElement>());
  const [placed, setPlaced] = useState<ReadonlyMap<string, Placement>>(new Map());
  const [height, setHeight] = useState(0);
  const seams = useRef(new Map<number, HTMLElement>());
  const [seamPad, setSeamPad] = useState<ReadonlyMap<number, number>>(new Map());
  // What the person has acknowledged in working memory: a log position, local to this view.
  const [seenThrough, setSeenThrough] = useState(-1);
  const lastEra = session.eras.length - 1;
  // Working memory is always on the right: a column when the row has room for it, a drawer when not.
  const root = useRef<HTMLDivElement>(null);
  const need = useRef<HTMLDivElement>(null);
  const [drawer, setDrawer] = useState(false);
  const [drawerOpen, setDrawerOpen] = useState(false);
  useLayoutEffect(() => {
    const el = root.current;
    const probe = need.current;
    if (!el || !probe) return;
    const fit = () => setDrawer(probe.offsetWidth > el.clientWidth);
    fit();
    const observer = new ResizeObserver(fit);
    observer.observe(el);
    return () => observer.disconnect();
  }, [lanes.length]);
  useEffect(() => {
    if (!drawer || !drawerOpen) return;
    const close = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setDrawerOpen(false);
    };
    window.addEventListener('keydown', close);
    return () => window.removeEventListener('keydown', close);
  }, [drawer, drawerOpen]);
  const liveEntries = session.memory.filter((m) => m.state === 'live').length;
  const unseen = session.memory.filter((m) => isUnseen(m, seenThrough)).length;

  // The clock: in the live app (`follow`), session time advances between events
  // while something runs, a tick a second; otherwise it is the last event's time.
  const receivedAt = useMemo(() => performance.now(), [session.events]);
  const running = session.state === 'turn' || session.state === 'capture' || session.state === 'ratify';
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!follow || !running) return;
    const id = setInterval(() => setTick((n) => n + 1), 1000);
    return () => clearInterval(id);
  }, [follow, running]);
  const now = follow && running ? session.now + (performance.now() - receivedAt) : session.now;

  const trunkOrder = session.eras.flatMap((era) => era.nodes.map((n) => n.id));
  const eraOf = new Map(session.eras.flatMap((era) => era.nodes.map((n) => [n.id, era.index] as const)));
  const laneBranches = (slot: number) =>
    trunkOrder.flatMap((id) => (session.branches.get(id) ?? []).filter((b) => b.slot === slot));

  const layout = useCallback(() => {
    const root = stage.current;
    if (!root) return;
    const base = root.getBoundingClientRect().top;
    // What the trunk drew, where, and when each node finished: the rule in placement.ts reads time down the page.
    const trunk: Span[] = session.eras.flatMap((era) =>
      era.nodes.flatMap((n) => {
        const r = anchors.current.get(n.id)?.getBoundingClientRect();
        return r ? [{ id: n.id, top: r.top - base, bottom: r.bottom - base, ...(n.endedAt !== undefined ? { endedAt: n.endedAt } : {}) }] : [];
      }),
    );
    const sides: Side[] = lanes.flatMap((slot) =>
      laneBranches(slot).flatMap((b) => {
        const cellEl = cells.current.get(b.id);
        if (!cellEl) return [];
        return [
          {
            id: b.id,
            at: b.at,
            slot,
            height: cellEl.offsetHeight,
            ...(b.startedAt !== undefined ? { startedAt: b.startedAt } : {}),
            ...(b.endedAt !== undefined ? { endedAt: b.endedAt } : {}),
          },
        ];
      }),
    );
    // In trunk order, whatever the slot: `place` orders them by when they started.
    const order = new Map(trunkOrder.map((id, i) => [id, i] as const));
    sides.sort((x, y) => (order.get(x.at) ?? 0) - (order.get(y.at) ?? 0));
    const spots = place(sides, trunk, session.now, gap);
    const next = new Map<string, Placement>();
    // The lowest branch bottom per era, for the seam that closes it.
    const eraBottom = new Map<number, number>();
    let bottom = 0;
    for (const side of sides) {
      const spot = spots.get(side.id);
      const cellEl = cells.current.get(side.id);
      const anchorEl = anchors.current.get(side.at);
      if (!spot || !cellEl || !anchorEl) continue;
      next.set(side.id, { ...spot, reach: cellEl.getBoundingClientRect().left - anchorEl.getBoundingClientRect().right });
      const era = eraOf.get(side.at) ?? 0;
      eraBottom.set(era, Math.max(eraBottom.get(era) ?? 0, spot.top + side.height));
      bottom = Math.max(bottom, spot.top + side.height + gap);
    }
    // A seam is a barrier: the refill happens after every side call before it
    // has finished, so it is drawn below all of them, and only it moves the trunk.
    const pads = new Map<number, number>();
    for (const [era, el] of seams.current) {
      // The wrapper's own top does not move with its padding: the pad is inside it.
      const natural = el.getBoundingClientRect().top - base;
      let before = 0;
      for (const [e, b] of eraBottom) if (e < era) before = Math.max(before, b);
      const pad = Math.max(0, Math.ceil(before + gap - natural));
      if (pad > 0) pads.set(era, pad);
    }
    setPlaced((prev) => (samePlacements(prev, next) ? prev : next));
    setHeight((prev) => (prev === bottom ? prev : bottom));
    setSeamPad((prev) => (sameNumbers(prev, pads) ? prev : pads));
    // `lanes`, `laneBranches` and `gap` are derived from `session` and `surface`.
  }, [session, surface.curtain, condensed]);

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
    if (revealed === undefined || condensed || scrolledTo.current === revealed || !placed.has(revealed)) return;
    scrolledTo.current = revealed;
    cells.current.get(revealed)?.scrollIntoView({ block: 'center' });
  }, [revealed, condensed, placed]);

  // Following: the bottom of the page is now -- the newest trunk node, or the
  // side calls queued past it -- so the view is locked to it while the page
  // grows, until the person scrolls away from it, and again once they return.
  const locked = useRef(true);
  useEffect(() => {
    if (!follow) return;
    const page = document.documentElement;
    const atBottom = () => window.innerHeight + window.scrollY >= page.scrollHeight - LOCK_SLACK;
    // Only the person moves the lock: scrolling up away from the bottom lets
    // go, scrolling down onto it takes hold. The page moves the view too --
    // scroll anchoring as a placement lands above it, clamping as it briefly
    // shrinks during a layout pass -- and none of that is someone reading.
    let last = window.scrollY;
    const onScroll = () => {
      const y = window.scrollY;
      if (y > last && atBottom()) locked.current = true;
      else if (y < last - 1 && !atBottom()) locked.current = false;
      last = y;
    };
    const stick = () => {
      if (!locked.current) return;
      window.scrollTo(0, page.scrollHeight);
      // Scroll events are coalesced a frame at a time: a person's scroll in the
      // same frame as this one must be measured from here, not from before it.
      last = window.scrollY;
    };
    window.addEventListener('scroll', onScroll, { passive: true });
    const observer = new ResizeObserver(stick);
    if (root.current) observer.observe(root.current);
    stick();
    return () => {
      window.removeEventListener('scroll', onScroll);
      observer.disconnect();
    };
  }, [follow]);

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
      <ClockContext.Provider value={now}>
      <div
        ref={root}
        className={`ex-session ex-session--minimap${surface.gaps ? ' ex-gaps' : ''}${surface.curtain ? ' ex-session--curtain' : ''}${condensed ? ' ex-session--condensed' : ''}`}
        style={{ ['--lanes' as string]: lanes.length, ...laneStyle(busyLane(session)) }}
        data-state={session.state}
        data-link={link}
        data-lanes-busy={session.occupancy.some((holder, slot) => holder !== undefined && slot !== session.trunkSlot) ? '' : undefined}
        data-fresh={unseen > 0 ? '' : undefined}
        data-drawer={drawer ? '' : undefined}
      >
        {/* As wide as the row must be for working memory to sit beside it (session.css, --need). */}
        <div className="ex-session__need" ref={need} aria-hidden="true" />
        <Minimap stage={stage} revision={[session, placed, seamPad, surface.curtain]} curtain={surface.curtain} />
        <div className="ex-session__header">
          <SessionHeader session={session} link={link} surface={surface} {...(onSurface ? { onSurface } : {})} />
        </div>
        <div className="ex-session__main">
          <div className="ex-session__columns">
            {lanes.length > 0 ? (
              <div className="ex-lanehead-row" aria-hidden="true">
                <div className="ex-lanehead">slot {session.trunkSlot} · trunk</div>
                {lanes.map((slot) => (
                  <div
                    className="ex-lanehead"
                    key={slot}
                    data-busy={session.occupancy[slot] === undefined ? undefined : ''}
                    data-lane={session.occupancy[slot]?.lane}
                    style={laneStyle(session.occupancy[slot]?.lane)}
                  >
                    <span className="ex-lanehead__long">slot </span>
                    {slot}
                    <span className="ex-lanehead__long"> · {session.occupancy[slot]?.id ?? 'idle'}</span>
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
                      condensed={condensed}
                      open={branch.id === revealed}
                      {...(onSurface
                        ? {
                            onOpen: () => {
                              setRevealed(branch.id);
                              scrolledTo.current = undefined;
                              onSurface({ ...surface, condensed: false });
                            },
                          }
                        : {})}
                    />
                  ))}
                </div>
              ))}
            </div>
            {/* The composer sits in the trunk's own grid column, so it lines up with the trunk at any width. */}
            <div className="ex-session__composer">
              <Composer key={session.phase} state={session.state} link={link} phase={session.phase} {...composer} />
            </div>
          </div>
          <aside className="ex-session__memory" data-open={drawer && drawerOpen ? '' : undefined}>
            {drawer ? (
              <button
                type="button"
                className="ex-drawer__tab"
                aria-expanded={drawerOpen}
                data-fresh={unseen > 0 ? '' : undefined}
                onClick={() => setDrawerOpen(!drawerOpen)}
              >
                working memory <span className="ex-drawer__count">{liveEntries}</span>
                {unseen > 0 ? <span className="ex-drawer__fresh">+{unseen}</span> : null}
              </button>
            ) : null}
            <div className="ex-drawer__body" inert={drawer && !drawerOpen}>
              <Memory entries={session.memory} seenThrough={seenThrough} onSeen={() => setSeenThrough(session.events - 1)} />
            </div>
          </aside>
        </div>
      </div>
      </ClockContext.Provider>
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
    case 'settled':
      return <TurnEnd node={node} />;
    default: {
      // Node kinds are the surface's own, a closed set: a new one must be drawn here.
      const unhandled: never = node;
      return unhandled;
    }
  }
}

function BranchCell({
  branch,
  placement,
  cellRef,
  evicted,
  condensed,
  open,
  onOpen,
}: {
  readonly branch: Folded<BranchNode>;
  readonly placement: Placement | undefined;
  readonly cellRef: (el: HTMLElement | null) => void;
  /** Its trunk node was evicted at a later seam. */
  readonly evicted: boolean;
  /** Drawn as a bar that keeps its place (the curtain's `condensed`). */
  readonly condensed: boolean;
  /** Opened when it is drawn whole: the person pressed its bar. */
  readonly open: boolean;
  readonly onOpen?: () => void;
}) {
  // Until the first layout pass the cell is measured in place, invisibly.
  const drop = placement ? placement.top + ENTER - placement.leave : 0;
  const pending = placement?.pending ?? false;
  return (
    <div
      className="ex-branchcell"
      ref={cellRef}
      style={{ ...laneStyle(branch.lane), ...(placement ? { top: placement.top } : { top: 0, visibility: 'hidden' }) }}
      data-branch={branch.id}
      data-evicted={evicted ? '' : undefined}
      data-pending={pending ? '' : undefined}
    >
      {placement ? <Cable reach={placement.reach} drop={drop} top={ENTER - drop} live={branch.outcome === undefined && !pending} pending={pending} /> : null}
      {condensed ? <BranchBar node={branch} {...(onOpen ? { onOpen } : {})} /> : <Branch node={branch} open={open} />}
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
    if (!q || q.pending !== p.pending || Math.abs(q.top - p.top) > 0.5 || Math.abs(q.leave - p.leave) > 0.5 || Math.abs(q.reach - p.reach) > 0.5) return false;
  }
  return true;
}

/** The lane of the first side call running now: the session's light while it runs. */
function busyLane(session: Session) {
  return session.occupancy.find((holder, slot) => holder !== undefined && slot !== session.trunkSlot)?.lane;
}

/** Every slot but the trunk's, in order. */
function laneSlots(session: Session): number[] {
  return Array.from({ length: session.slots }, (_, i) => i).filter((i) => i !== session.trunkSlot);
}

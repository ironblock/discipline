import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';

import type { Link } from '../drive/transport.ts';
import type { AssistantNode, BranchNode, Folded, Session, ToolNode, TrunkNode } from '../session/fold.ts';
import { Branch, BranchBar } from './Branch.tsx';
import { Cable } from './Cable.tsx';
import { Wiring } from './Wiring.tsx';
import type { Cabled } from './Wiring.tsx';
import { cabling, draw, HARNESS } from './harness.ts';
import type { Tap } from './harness.ts';
import { Links } from './Links.tsx';
import type { Wire } from './Links.tsx';
import { ENTER, place } from './placement.ts';
import type { Placed, Side, Span } from './placement.ts';
import { Composer } from './Composer.tsx';
import type { ComposerProps } from './Composer.tsx';
import { Memory, isUnseen } from './Memory.tsx';
import { chain } from './chain.ts';
import type { Pointed } from './chain.ts';
import { Minimap } from './Minimap.tsx';
import { wiringOf } from './prefs.ts';
import { usePrefs } from './Prefs.tsx';
import { AssistantMessage, SystemMessage, TurnEnd, UserMessage } from './Message.tsx';
import { Receipt } from './Receipt.tsx';
import { Seam } from './Seam.tsx';
import { SessionHeader } from './SessionHeader.tsx';
import { laneStyle } from './sets.ts';
import { ClockContext, HotEntriesContext, SurfaceContext, TargetContext } from './surface.tsx';
import type { Room, Surface } from './surface.tsx';
import { ToolBlock } from './ToolCall.tsx';
import './session.css';

export interface SessionViewProps {
  readonly session: Session;
  /** The connection to the drive. Absent: live. */
  readonly link?: Link;
  /** Why the link is not live, when the transport says (finding 17: a status the author must see). */
  readonly linkWhy?: string;
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
/** How long after a wheel, a touch or a key a scroll is still the person's: a trackpad's momentum, a smooth scroll's animation. */
const HAND_MS = 750;
/** Keys that scroll the page, when they are not typing. */
const SCROLL_KEYS: ReadonlySet<string> = new Set(['ArrowUp', 'ArrowDown', 'PageUp', 'PageDown', 'Home', 'End', ' ']);
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
export function SessionView({ session, link = 'live', linkWhy, surface, onSurface, composer, follow = false }: SessionViewProps) {
  // What the row has room for beside the trunk (measured below): whole side calls, bars, or neither. With
  // less room than the curtain asks for, side calls condense to bars; with none, the curtain draws closed.
  // Either way a side call that cannot open in its lane opens under the message it came from (`inline`).
  const [room, setRoom] = useState<Room>('whole');
  const [inline, setInline] = useState<ReadonlySet<string>>(new Set());
  // Where each side call opened under its message is: its cable leaves the trunk there, to its bar.
  const inlines = useRef(new Map<string, HTMLElement>());
  const slots = laneSlots(session);
  const curtain = surface.curtain && room !== 'none';
  const lanes = curtain ? slots : [];
  const condensed = curtain && (surface.condensed === true || room === 'bars');
  const opened = useMemo(() => (room === 'bars' ? [...inline] : []), [room, inline]);
  const gap = condensed ? STACK_GAP_CONDENSED : STACK_GAP;
  // A bar pressed while condensed: that side call, opened, once the curtain is.
  const [revealed, setRevealed] = useState<string>();
  const scrolledTo = useRef<string>(undefined);
  const stage = useRef<HTMLDivElement>(null);
  const anchors = useRef(new Map<string, HTMLElement>());
  const cells = useRef(new Map<string, HTMLElement>());
  const [placed, setPlaced] = useState<ReadonlyMap<string, Placement>>(new Map());
  // Where the trunk ends and each slot's gutter lies, in the stage's coordinates: for cables routed as a harness.
  const [columns, setColumns] = useState<Columns>();
  const [height, setHeight] = useState(0);
  const seams = useRef(new Map<number, HTMLElement>());
  const [seamPad, setSeamPad] = useState<ReadonlyMap<number, number>>(new Map());
  // What the person has acknowledged in working memory: a log position, local to this view.
  const [seenThrough, setSeenThrough] = useState(-1);
  const lastEra = session.eras.length - 1;
  // Working memory is always on the right: a column when the row has room for it, a drawer when not.
  const root = useRef<HTMLDivElement>(null);
  const need = useRef<HTMLDivElement>(null);
  const needWhole = useRef<HTMLDivElement>(null);
  const needBars = useRef<HTMLDivElement>(null);
  const [drawer, setDrawer] = useState(false);
  const [drawerOpen, setDrawerOpen] = useState(false);
  useLayoutEffect(() => {
    const el = root.current;
    const probe = need.current;
    if (!el || !probe) return;
    const fit = () => {
      const width = el.clientWidth;
      setDrawer(probe.offsetWidth > width);
      const whole = needWhole.current?.offsetWidth ?? 0;
      const bars = needBars.current?.offsetWidth ?? 0;
      setRoom(whole <= width ? 'whole' : bars <= width ? 'bars' : 'none');
    };
    fit();
    const observer = new ResizeObserver(fit);
    observer.observe(el);
    return () => observer.disconnect();
  }, [lanes.length, slots.length]);
  useEffect(() => {
    if (!drawer || !drawerOpen) return;
    const close = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setDrawerOpen(false);
    };
    window.addEventListener('keydown', close);
    return () => window.removeEventListener('keydown', close);
  }, [drawer, drawerOpen]);
  // Side calls opened under their message, all together or none: for a peek, every one off a message.
  const toggleInline = (ids: readonly string[]) =>
    setInline((was) => {
      const next = new Set(was);
      const opening = ids.some((id) => !was.has(id));
      for (const id of ids) {
        if (opening) next.add(id);
        else next.delete(id);
      }
      return next;
    });
  // The address's `#<id>`, if any (see the deep-link effect below).
  const [target, setTarget] = useState<string>();
  const wentTo = useRef<string>(undefined);
  // What each side call wrote, as wires into working memory; and what is lit.
  const wires = useMemo<readonly Wire[]>(
    () =>
      curtain
        ? [...session.branches.values()].flat().flatMap((b) => b.patches.map((p) => ({ branch: b.id, entry: p.entryId, lane: b.lane, op: p.op })))
        : [],
    [session, curtain],
  );
  // What is pointed at (or focused), and the chain it lights (chain.ts); the address's target when nothing is.
  const [pointed, setPointed] = useState<Pointed>({});
  const hot = useMemo(() => {
    const sideOf = new Map([...session.branches.values()].flat().map((b) => [b.id, b] as const));
    const aimed: Pointed =
      pointed.node !== undefined || pointed.branches !== undefined || pointed.entry !== undefined
        ? pointed
        : target === undefined
          ? // A side call opened under its message is lit, with its bar and what it wrote, until something else is pointed at.
            opened.length > 0
            ? { branches: opened }
            : {}
          : target.startsWith('memory/')
            ? { entry: target.slice('memory/'.length) }
            : sideOf.has(target)
              ? { branches: [target] }
              : session.branches.has(target)
                ? { node: target }
                : {};
    return chain(aimed, wires, (node) => (session.branches.get(node) ?? []).map((b) => b.id), (branch) => sideOf.get(branch)?.at);
  }, [wires, pointed, target, session, opened]);
  const pointAt = (el: Element | null) => {
    const next = el ? pointedAt(el) : {};
    if (!samePointed(pointed, next)) setPointed(next);
  };
  const liveEntries = session.memory.filter((m) => m.state === 'live').length;
  const unseen = session.memory.filter((m) => isUnseen(m, seenThrough)).length;

  // The clock: in the live app (`follow`), session time advances between events
  // while something runs, a tick a second; otherwise it is the last event's time.
  const receivedAt = useMemo(() => performance.now(), [session.events]);
  const running = session.state === 'turn' || session.state === 'capture' || session.state === 'ratify';
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!follow || !running) return;
    // Five times a second: running counts are said in tenths of a second.
    const id = setInterval(() => setTick((n) => n + 1), 200);
    return () => clearInterval(id);
  }, [follow, running]);
  const now = follow && running ? session.now + (performance.now() - receivedAt) : session.now;

  const trunkOrder = session.eras.flatMap((era) => era.nodes.map((n) => n.id));
  const eraOf = new Map(session.eras.flatMap((era) => era.nodes.map((n) => [n.id, era.index] as const)));
  const laneBranches = (slot: number) =>
    trunkOrder.flatMap((id) => (session.branches.get(id) ?? []).filter((b) => b.slot === slot));

  // The trunk's cables routed as a harness (harness.ts), when the surface asks.
  // How lines are drawn, and which: the person's preferences.
  const prefs = usePrefs();
  const wiring = curtain ? wiringOf(prefs) : undefined;
  const minimap = prefs.minimap === 'on';
  const cabled = useMemo<readonly Cabled[]>(() => {
    if (!wiring || !columns) return [];
    const byId = new Map(lanes.flatMap((slot) => laneBranches(slot).map((b) => [b.id, b] as const)));
    const taps: Tap[] = lanes.flatMap((slot) =>
      laneBranches(slot).flatMap((b) => {
        const p = placed.get(b.id);
        const gutter = columns.gutters.get(slot);
        return p && gutter ? [{ id: b.id, anchor: b.at, slot, leave: p.leave, enter: { x: gutter.right, y: p.top + ENTER }, pending: p.pending }] : [];
      }),
    );
    const options = { ...HARNESS, ...wiring };
    const routed = cabling(taps, columns.trunkRight, columns.gutters, options);
    const drawn = draw(routed, options);
    return routed.map((net, i) => {
      const lines = drawn[i];
      const first = byId.get(net.pins[0]?.entries[0] ?? '');
      return {
        key: net.key,
        node: first?.at ?? '',
        lane: first?.lane ?? '',
        pending: net.key.endsWith('>pending'),
        d: lines?.d ?? '',
        dots: lines?.dots ?? [],
        source: net.source,
        pins: net.pins,
        wires: (lines?.wires ?? []).map((w) => {
          const b = byId.get(w.entry);
          return { id: w.entry, lane: b?.lane ?? '', d: w.d, live: b?.outcome === undefined && placed.get(w.entry)?.pending === false };
        }),
      };
    });
    // `lanes` and `laneBranches` are derived from `session` and `surface`.
  }, [wiring, columns, placed, session, curtain]);

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
      // Opened under its message, it is where its cable leaves from, level with its header: one line from it to its bar.
      const opened = inlines.current.get(side.id);
      const leave = opened ? opened.getBoundingClientRect().top - base + ENTER : spot.leave;
      next.set(side.id, { ...spot, leave, reach: cellEl.getBoundingClientRect().left - anchorEl.getBoundingClientRect().right });
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
    const box = root.getBoundingClientRect();
    const trunkRight = (root.querySelector('.ex-trunk')?.getBoundingClientRect().right ?? box.left) - box.left;
    const gutters = new Map<number, { left: number; right: number }>();
    let before = trunkRight;
    for (const el of root.querySelectorAll<HTMLElement>('.ex-lane')) {
      const r = el.getBoundingClientRect();
      gutters.set(Number(el.dataset.slot), { left: before, right: r.left - box.left });
      before = r.right - box.left;
    }
    setColumns((prev) => (prev && sameColumns(prev, { trunkRight, gutters }) ? prev : { trunkRight, gutters }));
    setHeight((prev) => (prev === bottom ? prev : bottom));
    setSeamPad((prev) => (sameNumbers(prev, pads) ? prev : pads));
    // `lanes`, `laneBranches` and `gap` are derived from `session` and `surface`.
  }, [session, curtain, condensed]);

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

  // A deep link, `#<id>`: go to that node once it is drawn and placed (the
  // browser's own jump can come before either), and stop following. `:target`
  // is not enough to mark it (see TargetContext), so the target is state.
  useEffect(() => {
    const read = () => {
      wentTo.current = undefined;
      setTarget(decodeURIComponent(window.location.hash.slice(1)) || undefined);
    };
    read();
    window.addEventListener('hashchange', read);
    return () => window.removeEventListener('hashchange', read);
  }, []);
  useLayoutEffect(() => {
    if (target === undefined || wentTo.current === target) return;
    const el = document.getElementById(target);
    if (!el || el.closest('.ex-branchcell')?.getAttribute('style')?.includes('hidden')) return;
    wentTo.current = target;
    locked.current = false;
    el.scrollIntoView({ block: 'center' });
  }, [target, placed]);

  // Following: the bottom of the page is now -- the newest trunk node, or the
  // side calls queued past it -- so the view is locked to it while the page
  // grows, until the person scrolls away from it, and again once they return.
  const locked = useRef(true);
  useEffect(() => {
    if (!follow) return;
    const page = document.documentElement;
    const atBottom = () => window.innerHeight + window.scrollY >= page.scrollHeight - LOCK_SLACK;
    // Only the person lets go: scrolling up away from the bottom, with a hand
    // on it -- a wheel or a trackpad, a touch, a key that scrolls, the
    // scrollbar held. The page moves the view too -- scroll anchoring as a
    // placement lands above it, clamping as it briefly shrinks during a layout
    // pass -- and none of that is someone reading. Where the view was is not
    // enough to tell them apart: WebKit reports a clamp's scroll after the
    // page has grown back, so it looks like a scroll up away from the bottom.
    // Reaching the bottom, by any means, takes hold again.
    let last = window.scrollY;
    let touched = Number.NEGATIVE_INFINITY;
    let holding = false;
    const hand = () => {
      touched = performance.now();
    };
    const byHand = () => holding || performance.now() - touched < HAND_MS;
    const onKey = (e: KeyboardEvent) => {
      const typing = e.target instanceof HTMLElement && (e.target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(e.target.tagName));
      if (!typing && SCROLL_KEYS.has(e.key)) hand();
    };
    const onDown = () => {
      holding = true;
    };
    const onUp = () => {
      holding = false;
      hand();
    };
    const onScroll = () => {
      const y = window.scrollY;
      if (y > last && atBottom()) locked.current = true;
      else if (y < last - 1 && !atBottom() && byHand()) locked.current = false;
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
    window.addEventListener('wheel', hand, { passive: true });
    window.addEventListener('touchmove', hand, { passive: true });
    window.addEventListener('keydown', onKey);
    window.addEventListener('pointerdown', onDown);
    window.addEventListener('pointerup', onUp);
    window.addEventListener('pointercancel', onUp);
    const observer = new ResizeObserver(stick);
    if (root.current) observer.observe(root.current);
    stick();
    return () => {
      window.removeEventListener('scroll', onScroll);
      window.removeEventListener('wheel', hand);
      window.removeEventListener('touchmove', hand);
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('pointerdown', onDown);
      window.removeEventListener('pointerup', onUp);
      window.removeEventListener('pointercancel', onUp);
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
      <TargetContext.Provider value={target}>
      <HotEntriesContext.Provider value={hot.entries}>
      <div
        ref={root}
        className={`ex-session${minimap ? ' ex-session--minimap' : ''}${surface.gaps ? ' ex-gaps' : ''}${curtain ? ' ex-session--curtain' : ''}${condensed ? ' ex-session--condensed' : ''}`}
        style={{ ['--lanes' as string]: lanes.length, ['--slots' as string]: slots.length, ...laneStyle(busyLane(session)) }}
        data-room={room}
        data-state={session.state}
        data-link={link}
        data-lanes-busy={session.occupancy.some((holder, slot) => holder !== undefined && slot !== session.trunkSlot) ? '' : undefined}
        data-fresh={unseen > 0 ? '' : undefined}
        data-drawer={drawer ? '' : undefined}
        data-wiring={wiring ? 'harness' : undefined}
        onPointerOver={(e) => pointAt(e.target as Element)}
        onPointerLeave={() => pointAt(null)}
        onFocus={(e) => pointAt(e.target)}
        onBlur={(e) => {
          if (!e.currentTarget.contains(e.relatedTarget)) pointAt(null);
        }}
      >
        {prefs.memoryLines === 'on' ? <Links wires={wires} hot={hot} {...(wiring ? { wiring } : {})} revision={[session, placed, seamPad, drawer, drawerOpen, condensed]} /> : null}
        {/* As wide as the row must be for working memory to sit beside it (session.css, --need). */}
        <div className="ex-session__need" ref={need} aria-hidden="true" />
        <div className="ex-session__need" data-for="whole" ref={needWhole} aria-hidden="true" />
        <div className="ex-session__need" data-for="bars" ref={needBars} aria-hidden="true" />
        {minimap ? <Minimap stage={stage} revision={[session, placed, seamPad, curtain]} curtain={curtain} /> : null}
        <div className="ex-session__header">
          <SessionHeader session={session} link={link} {...(linkWhy !== undefined ? { linkWhy } : {})} surface={surface} room={room} {...(onSurface ? { onSurface } : {})} />
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
                    title={session.occupancy[slot] ? `slot ${slot}: ${session.occupancy[slot]?.lane} ${session.occupancy[slot]?.id}` : `slot ${slot}: idle`}
                  >
                    <span className="ex-lanehead__long">slot </span>
                    {slot}
                    <span className="ex-lanehead__long"> · {session.occupancy[slot]?.lane ?? 'idle'}</span>
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
                        style={curtain ? { paddingTop: seamPad.get(era.index) ?? 0 } : undefined}
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
                        <div
                          className="ex-trunk__node"
                          key={node.id}
                          ref={anchorRef(node.id)}
                          data-node={curtain && branches.length > 0 ? node.id : undefined}
                          data-hot={hot.nodes.has(node.id) ? '' : undefined}
                          data-kind={node.kind}
                        >
                          <TrunkBlock node={node} era={era.nodes} />
                          {!curtain && branches.length > 0 ? (
                            <button
                              type="button"
                              className="ex-peek"
                              title={`${branches.length} side call${branches.length === 1 ? '' : 's'} off this message`}
                              aria-expanded={room === 'none' ? branches.some((b) => inline.has(b.id)) : undefined}
                              onClick={() => (room === 'none' ? toggleInline(branches.map((b) => b.id)) : onSurface?.({ ...surface, curtain: true }))}
                            >
                              ↳ {branches.length}
                            </button>
                          ) : null}
                          {room !== 'whole'
                            ? branches
                                .filter((b) => inline.has(b.id))
                                .map((b) => (
                                  <div
                                    className="ex-inline"
                                    key={b.id}
                                    style={laneStyle(b.lane)}
                                    data-branch-inline={b.id}
                                    ref={(el) => {
                                      if (el) inlines.current.set(b.id, el);
                                      else inlines.current.delete(b.id);
                                    }}
                                  >
                                    <Branch node={b} open />
                                  </div>
                                ))
                            : null}
                        </div>
                      );
                    })}
                  </section>
                ))}
              </div>
              {cabled.length > 0 ? <Wiring nets={cabled} lit={hot.branches} /> : null}
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
                      wired={wiring !== undefined}
                      hot={hot.branches.has(branch.id)}
                      open={branch.id === revealed}
                      {...(room === 'bars'
                        ? { onOpen: () => toggleInline([branch.id]) }
                        : onSurface
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
            {/* The composer: its own width from the trunk's left edge, whatever the lanes (--composer-width). */}
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
              <Receipt receipt={session.receipt} />
            </div>
          </aside>
        </div>
      </div>
      </HotEntriesContext.Provider>
      </TargetContext.Provider>
      </ClockContext.Provider>
    </SurfaceContext.Provider>
  );
}

/**
 * One node of the trunk. A tool node is a call and what it printed
 * (ToolBlock); the assistant message that wrote the call is found by it, as the calls it
 * ended in are found by the message, among the era's nodes.
 */
function TrunkBlock({ node, era }: { readonly node: TrunkNode; readonly era: readonly TrunkNode[] }) {
  switch (node.kind) {
    case 'user':
      return <UserMessage node={node} />;
    case 'assistant':
      return <AssistantMessage node={node} calls={era.filter((n): n is Folded<ToolNode> => n.kind === 'tool' && n.after === node.id)} />;
    case 'tool': {
      const caller = era.find((n): n is Folded<AssistantNode> => n.kind === 'assistant' && n.id === node.after);
      const first = era.find((n) => n.kind === 'tool' && n.after === node.after) === node;
      return <ToolBlock node={node} caller={caller} first={first} />;
    }
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
  wired,
  hot,
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
  /** Its cable is drawn with the others, routed as a harness (Wiring.tsx), not here. */
  readonly wired: boolean;
  /** Lit: it, or an entry it wrote, is pointed at. */
  readonly hot: boolean;
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
      data-hot={hot ? '' : undefined}
    >
      {placement && !wired ? (
        <Cable reach={placement.reach} drop={drop} top={ENTER - drop} live={branch.outcome === undefined && !pending} pending={pending} hot={hot} point={branch.id} />
      ) : null}
      {condensed ? <BranchBar node={branch} {...(onOpen ? { onOpen } : {})} /> : <Branch node={branch} open={open} />}
    </div>
  );
}

interface Columns {
  /** The trunk's right edge. */
  readonly trunkRight: number;
  /** Per slot, the gutter before its column. */
  readonly gutters: ReadonlyMap<number, { readonly left: number; readonly right: number }>;
}

function sameColumns(a: Columns, b: Columns): boolean {
  if (Math.abs(a.trunkRight - b.trunkRight) > 0.5 || a.gutters.size !== b.gutters.size) return false;
  for (const [slot, g] of b.gutters) {
    const h = a.gutters.get(slot);
    if (!h || Math.abs(h.left - g.left) > 0.5 || Math.abs(h.right - g.right) > 0.5) return false;
  }
  return true;
}

/**
 * What an element under the pointer (or focused) points at: a connector
 * names its ends (`data-point`); otherwise a side call, a trunk node that
 * has side calls, or a working-memory entry.
 */
function pointedAt(el: Element): Pointed {
  const hit = el.closest('[data-point]');
  if (hit) {
    const node = hit.getAttribute('data-node');
    const branches = hit.getAttribute('data-branches');
    const entry = hit.getAttribute('data-entry');
    return { ...(node ? { node } : {}), ...(branches ? { branches: branches.split(' ') } : {}), ...(entry ? { entry } : {}) };
  }
  const side = el.closest('.ex-branchcell')?.getAttribute('data-branch');
  if (side) return { branches: [side] };
  const node = el.closest('.ex-trunk__node[data-node]')?.getAttribute('data-node');
  if (node) return { node };
  const entry = el.closest('.ex-memory__entry')?.id.replace(/^memory\//, '');
  return entry ? { entry } : {};
}

function samePointed(a: Pointed, b: Pointed): boolean {
  return a.node === b.node && a.entry === b.entry && (a.branches ?? []).join(' ') === (b.branches ?? []).join(' ');
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

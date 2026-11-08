import { memo, useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { PointerEvent, RefObject } from 'react';

import { dragged, jump, picture } from './minimap.ts';
import type { Picture } from './minimap.ts';
import { laneStyle } from './sets.ts';
import './minimap.css';

interface Mark {
  readonly kind: 'trunk' | 'lane' | 'meditation';
  readonly top: number;
  readonly height: number;
  readonly tone?: string;
  readonly lane?: string;
  readonly alarm?: string;
  readonly live: boolean;
}

interface Measured {
  readonly marks: readonly Mark[];
  /** The stage's height, and the widths `picture` scales by: the trunk's, and a trunk mark's. */
  readonly height: number;
  readonly column: number;
  readonly mark: number;
}

/** No picture yet: nothing measured. */
const UNMEASURED: Picture = { scale: 0, offset: 0, lens: { top: 0, height: 0 }, ratio: 0 };

/**
 * The whole session at a glance, down the left edge: every trunk message and
 * every side call as a sliver in its own colour, each refill as a line across,
 * whatever went wrong as a tick in its alarm colour, what is running now lit,
 * and a lens over what is in view. Press to go there; drag the lens to scroll.
 *
 * It draws as VS Code's proportional minimap does (`picture`): one scale for
 * both axes, so a block keeps its message's shape; a session taller than the
 * map scrolls in step with the page, the lens at the page's relative place
 * over the blocks the page shows.
 *
 * It draws what the page has laid out -- it measures the stage, it does not
 * re-derive the layout -- and reads only what blocks already say about
 * themselves: `data-tone`, `data-lane`, `data-alarm`, and whether they are live.
 */
export function Minimap({ stage, revision, curtain }: { readonly stage: RefObject<HTMLElement | null>; readonly revision: readonly unknown[]; readonly curtain: boolean }) {
  const map = useRef<HTMLDivElement>(null);
  // A trunk mark's width, measured, never derived from the stylesheet's percentages.
  const probe = useRef<HTMLSpanElement>(null);
  const [measured, setMeasured] = useState<Measured>({ marks: [], height: 0, column: 0, mark: 0 });
  // The geometry and the picture, for a scroll and a press, which run apart from a re-render.
  const geometry = useRef(measured);
  const [view, setView] = useState<Picture>(UNMEASURED);
  const shown = useRef<Picture>(UNMEASURED);
  // A lens drag: where the pointer and the page were when it began.
  const drag = useRef<{ readonly y: number; readonly scroll: number } | null>(null);
  const frame = useRef(0);

  // The header and the composer bound what is in view. Found once and kept: this runs every frame of a
  // scroll, and the composer, last on the page, is a search of all of it (scripts/perf.mjs).
  const chrome = useRef<{ header?: Element | null | undefined; composer?: Element | null | undefined }>({});
  const band = useCallback(() => {
    const kept = chrome.current;
    if (!kept.header?.isConnected || !kept.composer?.isConnected) {
      const session = stage.current?.closest('.ex-session');
      kept.header = session?.querySelector('.ex-session__header');
      kept.composer = session?.querySelector('.ex-session__composer');
    }
    const header = kept.header?.getBoundingClientRect().bottom ?? 0;
    const composer = kept.composer?.getBoundingClientRect().top ?? window.innerHeight;
    return { top: header, bottom: Math.max(header + 1, Math.min(composer, window.innerHeight)) };
  }, [stage]);

  const look = useCallback(() => {
    const root = stage.current;
    if (!root) return;
    const rect = root.getBoundingClientRect();
    const { top, bottom } = band();
    const { height, column, mark } = geometry.current;
    const next = picture(height, column, mark, map.current?.clientHeight ?? 0, rect.top, top, bottom);
    shown.current = next;
    setView(next);
  }, [stage, band]);

  const remeasure = useCallback(() => {
    cancelAnimationFrame(frame.current);
    frame.current = requestAnimationFrame(() => {
      const root = stage.current;
      if (!root) return;
      const next = measure(root, probe.current);
      geometry.current = next;
      setMeasured(next);
      look();
    });
  }, [stage, look]);

  // The page changed: a new event, a branch placed, the curtain. `revision` is
  // the caller's list of what changes the layout, always the same length.
  useLayoutEffect(remeasure, [remeasure, ...revision]);

  useEffect(() => {
    const root = stage.current;
    if (!root) return;
    const observer = new ResizeObserver(remeasure);
    observer.observe(root);
    // The map's own height sets how far a long stage's picture scrolls.
    if (map.current) observer.observe(map.current);
    // One look a frame, however many scroll events the frame had.
    const onScroll = () => {
      cancelAnimationFrame(frame.current);
      frame.current = requestAnimationFrame(look);
    };
    window.addEventListener('scroll', onScroll, { passive: true });
    window.addEventListener('resize', onScroll);
    return () => {
      observer.disconnect();
      window.removeEventListener('scroll', onScroll);
      window.removeEventListener('resize', onScroll);
      cancelAnimationFrame(frame.current);
    };
  }, [stage, remeasure, look]);

  // A press goes to the point under it, centred, as VS Code's fill mode does -- then, held, drags the lens from
  // there; a press on the lens drags it as it stands. A drag moves the page by the lens's ratio (`dragged`).
  const press = (e: PointerEvent<HTMLDivElement>) => {
    const root = stage.current;
    const box = map.current?.getBoundingClientRect();
    const at = shown.current;
    if (!root || !box || box.height <= 0 || at.scale <= 0) return;
    const y = e.clientY - box.top;
    if (y < at.lens.top || y > at.lens.top + at.lens.height) {
      const rect = root.getBoundingClientRect();
      const { top, bottom } = band();
      const point = (y + at.offset) / at.scale / Math.max(1, rect.height);
      window.scrollTo({ top: jump(point, window.scrollY, rect.top, rect.height, top, bottom) });
    }
    drag.current = { y: e.clientY, scroll: window.scrollY };
  };
  const move = (e: PointerEvent<HTMLDivElement>) => {
    const from = drag.current;
    if (!from) return;
    window.scrollTo({ top: Math.max(0, from.scroll + dragged(shown.current, e.clientY - from.y)) });
  };

  return (
    <div
      className="ex-minimap"
      ref={map}
      data-curtain={curtain ? '' : undefined}
      aria-hidden="true"
      onPointerDown={(e) => {
        e.currentTarget.setPointerCapture(e.pointerId);
        press(e);
      }}
      onPointerMove={move}
      onPointerUp={() => {
        drag.current = null;
      }}
      onPointerCancel={() => {
        drag.current = null;
      }}
    >
      <span className="ex-mm__probe" ref={probe} />
      {/* The picture scrolls by its offset; the marks inside move only when the page is re-measured (scripts/perf.mjs). */}
      <div className="ex-mm__picture" style={{ transform: `translateY(${-view.offset}px)` }}>
        <Marks measured={measured} scale={view.scale} />
      </div>
      <span className="ex-mm__lens" style={{ top: `${view.lens.top}px`, height: `${view.lens.height}px` }} />
    </div>
  );
}

/**
 * The marks, apart from the lens: a scroll moves only the lens, and must not
 * re-render every sliver of a long session with it (scripts/perf.mjs).
 */
const Marks = memo(function Marks({ measured, scale }: { readonly measured: Measured; readonly scale: number }) {
  return measured.marks.map((mark, i) => {
    const at = { top: mark.top * scale, height: mark.height * scale };
    return (
      <span
        key={i}
        className={`ex-mm__${mark.kind}`}
        data-tone={mark.tone}
        data-alarm={mark.alarm}
        data-live={mark.live ? '' : undefined}
        style={{ ...laneStyle(mark.lane), top: `${at.top}px`, height: `${at.height}px` }}
      />
    );
  });
});

function measure(stage: HTMLElement, probe: HTMLElement | null): Measured {
  const base = stage.getBoundingClientRect();
  const span = (el: Element) => {
    const r = el.getBoundingClientRect();
    return { top: r.top - base.top, height: r.height };
  };
  const marks: Mark[] = [];
  for (const el of stage.querySelectorAll('.ex-trunk .ex-block, .ex-trunk .ex-turnend')) {
    const tone = el.getAttribute('data-tone') ?? 'turnend';
    const alarm = el.getAttribute('data-alarm') ?? undefined;
    marks.push({ kind: 'trunk', ...span(el), tone, ...(alarm ? { alarm } : {}), live: el.classList.contains('ex-block--live') });
  }
  // A refill is a line across, at the meditation itself (not the padding above it).
  for (const el of stage.querySelectorAll('.ex-era__meditation .ex-meditation')) marks.push({ kind: 'meditation', top: span(el).top + span(el).height / 2, height: 0, live: false });
  for (const el of stage.querySelectorAll('.ex-branchcell')) {
    // Open, a side call is a block; condensed, a bar -- which says its lane, alarm and liveness the same way, bar the class.
    const bar = el.querySelector('.ex-block, .ex-bar');
    const lane = bar?.getAttribute('data-lane') ?? undefined;
    const alarm = bar?.getAttribute('data-alarm') ?? undefined;
    const live = bar ? bar.classList.contains('ex-block--live') || bar.hasAttribute('data-live') : false;
    marks.push({ kind: 'lane', ...span(el), ...(lane ? { lane } : {}), ...(alarm ? { alarm } : {}), live });
  }
  const column = stage.querySelector('.ex-trunk')?.getBoundingClientRect().width ?? 0;
  return { marks, height: base.height, column, mark: probe?.getBoundingClientRect().width ?? 0 };
}

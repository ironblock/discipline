import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { PointerEvent, RefObject } from 'react';

import { jump, lens, onMap } from './minimap.ts';
import { laneStyle } from './sets.ts';
import './minimap.css';

interface Mark {
  readonly kind: 'trunk' | 'lane' | 'seam';
  readonly top: number;
  readonly height: number;
  readonly tone?: string;
  readonly lane?: string;
  readonly alarm?: string;
  readonly live: boolean;
}

interface Measured {
  readonly marks: readonly Mark[];
  readonly height: number;
}

/**
 * The whole session at a glance, down the left edge: every trunk message and
 * every side call as a sliver in its own colour, each refill as a line across,
 * whatever went wrong as a tick in its alarm colour, what is running now lit,
 * and a lens over what is in view. Press or drag to go there.
 *
 * It draws what the page has laid out -- it measures the stage, it does not
 * re-derive the layout -- and reads only what blocks already say about
 * themselves: `data-tone`, `data-lane`, `data-alarm`, and whether they are live.
 */
export function Minimap({ stage, revision, curtain }: { readonly stage: RefObject<HTMLElement | null>; readonly revision: readonly unknown[]; readonly curtain: boolean }) {
  const map = useRef<HTMLDivElement>(null);
  const [measured, setMeasured] = useState<Measured>({ marks: [], height: 0 });
  const [view, setView] = useState({ top: 0, height: 1 });
  const frame = useRef(0);

  const band = useCallback(() => {
    const session = stage.current?.closest('.ex-session');
    const header = session?.querySelector('.ex-session__header')?.getBoundingClientRect().bottom ?? 0;
    const composer = session?.querySelector('.ex-session__composer')?.getBoundingClientRect().top ?? window.innerHeight;
    return { top: header, bottom: Math.max(header + 1, Math.min(composer, window.innerHeight)) };
  }, [stage]);

  const look = useCallback(() => {
    const root = stage.current;
    if (!root) return;
    const rect = root.getBoundingClientRect();
    const { top, bottom } = band();
    setView(lens(rect.top, rect.height, top, bottom));
  }, [stage, band]);

  const remeasure = useCallback(() => {
    cancelAnimationFrame(frame.current);
    frame.current = requestAnimationFrame(() => {
      const root = stage.current;
      if (!root) return;
      setMeasured(measure(root));
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
    const onScroll = () => requestAnimationFrame(look);
    window.addEventListener('scroll', onScroll, { passive: true });
    window.addEventListener('resize', onScroll);
    return () => {
      observer.disconnect();
      window.removeEventListener('scroll', onScroll);
      window.removeEventListener('resize', onScroll);
      cancelAnimationFrame(frame.current);
    };
  }, [stage, remeasure, look]);

  const go = (e: PointerEvent<HTMLDivElement>) => {
    const root = stage.current;
    const box = map.current?.getBoundingClientRect();
    if (!root || !box || box.height <= 0) return;
    const rect = root.getBoundingClientRect();
    const { top, bottom } = band();
    window.scrollTo({ top: jump((e.clientY - box.top) / box.height, window.scrollY, rect.top, rect.height, top, bottom) });
  };

  return (
    <div
      className="ex-minimap"
      ref={map}
      data-curtain={curtain ? '' : undefined}
      aria-hidden="true"
      onPointerDown={(e) => {
        e.currentTarget.setPointerCapture(e.pointerId);
        go(e);
      }}
      onPointerMove={(e) => {
        if (e.currentTarget.hasPointerCapture(e.pointerId)) go(e);
      }}
    >
      {measured.marks.map((mark, i) => {
        const at = onMap(mark, measured.height);
        return (
          <span
            key={i}
            className={`ex-mm__${mark.kind}`}
            data-tone={mark.tone}
            data-alarm={mark.alarm}
            data-live={mark.live ? '' : undefined}
            style={{ ...laneStyle(mark.lane), top: `${at.top * 100}%`, height: `${at.height * 100}%` }}
          />
        );
      })}
      <span className="ex-mm__lens" style={{ top: `${view.top * 100}%`, height: `${view.height * 100}%` }} />
    </div>
  );
}

function measure(stage: HTMLElement): Measured {
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
  // A refill is a line across, at the seam itself (not the padding above it).
  for (const el of stage.querySelectorAll('.ex-era__seam .ex-seam')) marks.push({ kind: 'seam', top: span(el).top + span(el).height / 2, height: 0, live: false });
  for (const el of stage.querySelectorAll('.ex-branchcell')) {
    const bar = el.querySelector('.ex-block');
    const lane = bar?.getAttribute('data-lane') ?? undefined;
    const alarm = bar?.getAttribute('data-alarm') ?? undefined;
    marks.push({ kind: 'lane', ...span(el), ...(lane ? { lane } : {}), ...(alarm ? { alarm } : {}), live: bar?.classList.contains('ex-block--live') ?? false });
  }
  return { marks, height: base.height };
}

import type { ProgressFrame, Response } from './script.ts';

type Unplaced = Omit<ProgressFrame, 'seq'>;

/** How often a synthesized frame is emitted, ms: a stream's progress is finer, `/slots` polled once a second coarser. */
export const FRAME_MS = 250;

/**
 * Progress frames for a request that has already been answered, made from
 * its response's timings -- the specimen and the recordings have no frames of
 * their own -- in the shape the log writes them (`script.ts`): prefill only,
 * from the request to the first token. `processed` starts at the warm part
 * (the cache counts in, as llama.cpp's does) and fills the new part at an even
 * rate over `prompt_ms`; `time_ms` is the time since the request. Nothing is
 * framed after that, as nothing is in a served log: what is generated is the
 * response's to say. `every` is the frame period: a recording, refolded per
 * event, is given `/slots`'s once a second.
 */
export function frames(response: Omit<Response, 'seq'>, requested: number, done: number, every = FRAME_MS): Unplaced[] {
  const { prompt_n, cache_n, prompt_ms } = response.timings;
  const read = Math.min(done, requested + prompt_ms);
  const frame = (t: number, fresh: number): Unplaced => ({
    kind: 'progress',
    t,
    request: response.to_request,
    total: prompt_n + cache_n,
    cache: cache_n,
    processed: cache_n + fresh,
    time_ms: t - requested,
  });
  const out: Unplaced[] = [];
  for (let t = requested; t < read; t += every) out.push(frame(t, Math.round((prompt_n * (t - requested)) / Math.max(1, read - requested))));
  out.push(frame(read, prompt_n));
  return out;
}

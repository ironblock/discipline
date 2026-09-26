import type { ProgressFrame, Response } from './events.ts';

type Unplaced = Omit<ProgressFrame, 'seq'>;

/** How often a synthesized frame is emitted, ms: a stream's progress is finer, `/slots` polled once a second coarser. */
export const FRAME_MS = 250;

/**
 * Progress frames for a request that has already been answered, made from
 * its response's timings -- the specimen and the recordings have no frames of
 * their own. The new part of the prompt fills at an even rate over
 * `prompt_ms` from the request, the warm part there from the first frame;
 * then tokens are counted up to `predicted_n` by the response. `every` is
 * the frame period: a recording, refolded per event, is given `/slots`'s
 * once a second.
 */
export function frames(response: Omit<Response, 'seq'>, requested: number, done: number, every = FRAME_MS): Unplaced[] {
  const { prompt_n, cache_n, prompt_ms, predicted_n } = response.timings;
  const read = Math.min(done, requested + prompt_ms);
  const frame = (t: number, processed: number, decoded: number): Unplaced => ({
    kind: 'progress',
    t,
    request: response.to_request,
    prompt: { total: prompt_n + cache_n, cache: cache_n, processed },
    decoded,
  });
  const out: Unplaced[] = [];
  for (let t = requested; t < read; t += every) out.push(frame(t, Math.round((prompt_n * (t - requested)) / Math.max(1, read - requested)), 0));
  out.push(frame(read, prompt_n, 0));
  for (let t = read + every; t < done; t += every) out.push(frame(t, prompt_n, Math.round((predicted_n * (t - read)) / Math.max(1, done - read))));
  if (done > read) out.push(frame(done, prompt_n, predicted_n));
  return out;
}

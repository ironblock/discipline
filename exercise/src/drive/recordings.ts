import { load } from './recorded.ts';
import firstDrive from './recorded/first-drive.json?raw';
import cancelledCapture from './recorded/cancelled-capture.json?raw';
import stepLimit from './recorded/step-limit.json?raw';
import voxelStress from './recorded/voxel-stress.json?raw';

/**
 * Every recording, read at import: the app, the stories and the tests. Not
 * the replay page, which loads the one it shows as a file of its own
 * (`replay.tsx`): importing this module carries all four, inlined.
 */
export const RECORDINGS = {
  'first-drive': load('first-drive', firstDrive),
  /** A capture round the person cancelled: `capture.cancelled`, which the vocabulary does not have yet. */
  'cancelled-capture': load('cancelled-capture', cancelledCapture),
  /** A turn that ran into the step limit (30 steps). */
  'step-limit': load('step-limit', stepLimit),
  /** OpenCode, native tool calls, several per step, six tools; its side calls authored (stitch-sides.py). */
  'voxel-stress': load('voxel-stress', voxelStress),
} as const;

export type RecordingName = keyof typeof RECORDINGS;

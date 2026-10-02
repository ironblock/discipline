import type { ExampleName } from '../replay/published.ts';
import { KITCHEN_SINK } from './kitchen-sink.ts';
import type { Recording } from './recorded.ts';

/**
 * Each published example's source (#272): authored here, in TypeScript, and
 * committed as src/drive/examples/<name>.json by scripts/write-examples.mjs,
 * which is what the replay page publishes and what was admitted. The JSON is
 * this, serialized; published.test.ts fails when the two differ.
 */
export const AUTHORED: Readonly<Record<ExampleName, Recording>> = {
  'kitchen-sink': KITCHEN_SINK,
};

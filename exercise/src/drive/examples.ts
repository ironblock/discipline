import type { ExampleName } from '../replay/published.ts';
import kitchenSink from './examples/kitchen-sink.json?raw';
import { examplePath, load } from './recorded.ts';
import type { Recording } from './recorded.ts';

/**
 * Each published example (#272), read from its committed file: the file the replay page publishes and
 * `scripts/admission.py admit <name>` admits. The kitchen sink is what `diet/scenarios/kitchen-sink.jsonl` writes
 * (#798), written there by `node exercise/scripts/kitchen-sink.mjs write` and held to the scenario by its `check`.
 */
export const AUTHORED: Readonly<Record<ExampleName, Recording>> = {
  'kitchen-sink': load('kitchen-sink', kitchenSink, examplePath('kitchen-sink')),
};

/**
 * How a story reaches a record: by name, through the loader, never by hand.
 *
 * Every Sequences and Rows story starts here, so the code path a story
 * exercises is the one the app will have -- `diet`'s projection into
 * `load()`, `groupEvents()` over the result, a row bound from a group. A
 * story that assembled its own `Placed` events would test a path the app
 * does not have, and would be a hand-authored record wearing a fixture's
 * name.
 */

import { groupEvents } from '../record/group.ts';
import type { Group } from '../record/group.ts';
import { mustLoad } from '../record/load.ts';
import type { Loaded } from '../record/load.ts';
import { fixtures } from './generated/index.ts';
import type { FixtureName } from './generated/index.ts';

export type { FixtureName };
export { fixtureNames } from './generated/index.ts';

/** The fixture, loaded the way the app loads a record. */
export function loadFixture(name: FixtureName): Loaded {
  return mustLoad(fixtures[name].projection);
}

/** The fixture's rows. */
export function groupsOf(name: FixtureName): readonly Group[] {
  return groupEvents(loadFixture(name).events);
}

/** The `nth` group of `kind` in the fixture, or a thrown error naming what was asked. */
export function pickGroup<K extends Group['kind']>(name: FixtureName, kind: K, nth = 0): Extract<Group, { kind: K }> {
  const found = groupsOf(name).filter((g): g is Extract<Group, { kind: K }> => g.kind === kind);
  const group = found[nth];
  if (!group) throw new Error(`${name} has ${found.length} ${kind} group(s); asked for #${nth}`);
  return group;
}

/** What the fixture's decode lost, if anything. */
export function lossyOf(name: FixtureName) {
  return fixtures[name].lossy;
}

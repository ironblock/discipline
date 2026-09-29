/**
 * What lights together when a person points at one thing: the CHAIN it is
 * part of, trunk node -> side call -> working-memory entry, along the
 * connectors between them. Pure, so it is tested without a page.
 *
 * Pointing at a trunk node lights its side calls and what they wrote; at a
 * side call, the node it came from and what it wrote; at an entry, the side
 * calls that wrote it and the nodes they came from. A connector lights the
 * two things it joins and their chain: a trunk net, its node and the side
 * calls on it; a line into memory, its side call and that one entry.
 */

/** A patch as a line: the side call that landed it, and the entry it touched. */
export interface Written {
  readonly branch: string;
  readonly entry: string;
}

/** What is pointed at: a trunk node, some side calls, an entry -- or a connector, which names its ends. */
export interface Pointed {
  readonly node?: string;
  readonly branches?: readonly string[];
  readonly entry?: string;
}

export interface Chain {
  readonly nodes: ReadonlySet<string>;
  readonly branches: ReadonlySet<string>;
  readonly entries: ReadonlySet<string>;
  /** Lines into memory lit, as `branch>entry`. */
  readonly lines: ReadonlySet<string>;
}

const UNLIT: Chain = { nodes: new Set(), branches: new Set(), entries: new Set(), lines: new Set() };

export function lineKey(branch: string, entry: string): string {
  return `${branch}>${entry}`;
}

/**
 * The chain through what is pointed at. `sidesOf` gives a trunk node's side
 * calls; `nodeOf`, the node a side call came from.
 */
export function chain(pointed: Pointed, written: readonly Written[], sidesOf: (node: string) => readonly string[], nodeOf: (branch: string) => string | undefined): Chain {
  const { node, entry } = pointed;
  // The side calls in the chain: named, or a node's, or an entry's writers.
  const branches = new Set(pointed.branches ?? (node !== undefined ? sidesOf(node) : entry !== undefined ? written.filter((w) => w.entry === entry).map((w) => w.branch) : []));
  if (branches.size === 0 && node === undefined && entry === undefined) return UNLIT;
  // What they wrote: all of it, unless one entry was pointed at.
  const lines = written.filter((w) => branches.has(w.branch) && (entry === undefined || w.entry === entry));
  const nodes = new Set([...branches].flatMap((b) => nodeOf(b) ?? []));
  if (node !== undefined) nodes.add(node);
  return {
    nodes,
    branches,
    entries: new Set(entry !== undefined ? [entry] : lines.map((w) => w.entry)),
    lines: new Set(lines.map((w) => lineKey(w.branch, w.entry))),
  };
}

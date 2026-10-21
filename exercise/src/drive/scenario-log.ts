import type { LogLine } from './log.ts';

/**
 * The kitchen sink's log as `diet-drive scenario` writes it (#798): scrubbed of the run's own paths before it is
 * published, and compared with what the scenario writes today, lane by lane. `scripts/kitchen-sink.mjs` writes and
 * checks with these.
 */

/** A run's temporary directories (macOS's per-user temp, or /tmp), named for what the scenario made them. */
const TEMP_DIR = /(?:(?:\/private)?\/var\/folders\/[^/\s"'<>]+\/[^/\s"'<>]+\/T|\/tmp)\/diet-([a-z]+(?:-[a-z]+)*)-\d+-\d+/g;
/** A home directory: the account's name is the part replaced. */
const HOME_DIR = new RegExp('/(?:Users|home)/[^/\\s"\'<>]+', 'g');
/** A background job's id: random per run. */
const JOB = /\bbg_[0-9a-f]{8}\b/g;
/** A process id, as a background job's start and its notice say it: the system's, per run. */
const PID = /(<pid>|\bpid: )(\d+)/g;

/** Every string in the log scrubbed: each path the caller names to what it was (the worktree the run was given), temp
 * directories to `/work/<what it was>`, home directories to `/work/user`, and each background job's id and process id
 * to its place in the run (`bg_1`, `<pid>1</pid>`, …), wherever written. */
export function scrub(lines: readonly LogLine[], named: Readonly<Record<string, string>> = {}): LogLine[] {
  const given = Object.entries(named).sort(([a], [b]) => b.length - a.length);
  const jobs = new Map<string, string>();
  const pids = new Map<string, number>();
  const text = (value: string) =>
    given
      .reduce((out, [path, as]) => out.split(path).join(as), value)
      .replace(TEMP_DIR, (_, what: string) => `/work/${what.replace(/^diet-/, '')}`)
      .replace(HOME_DIR, '/work/user')
      .replace(JOB, (id) => {
        if (!jobs.has(id)) jobs.set(id, `bg_${jobs.size + 1}`);
        return jobs.get(id)!;
      })
      .replace(PID, (_, before: string, pid: string) => {
        if (!pids.has(pid)) pids.set(pid, pids.size + 1);
        return `${before}${pids.get(pid)!}`;
      });
  const walk = (value: unknown): unknown =>
    typeof value === 'string'
      ? text(value)
      : Array.isArray(value)
        ? value.map(walk)
        : value !== null && typeof value === 'object'
          ? Object.fromEntries(Object.entries(value).map(([key, inner]) => [key, walk(inner)]))
          : value;
  return lines.map((line) => walk(line) as LogLine);
}

/** What a scrubbed text must not hold: a home or temp path, and whatever else the caller names (this machine's user
 * and host, read where the scrub runs). Each match, in order. */
export function residue(text: string, also: readonly string[] = []): string[] {
  const shapes = [/(?:\/private)?\/var\/folders\/[^\s"'<>]+/g, /\/private\/var\/[^\s"'<>]+/g, /\/tmp\/[^\s"'<>]+/g, HOME_DIR];
  const found = shapes.flatMap((shape) => [...text.matchAll(new RegExp(shape.source, 'g'))].map((m) => m[0]));
  return [...new Set(found), ...also.filter((word) => word !== '' && text.includes(word))];
}

/** Fields that name another line by its `seq`: compared as the target's lane and place in it, never as a number. */
const REFERENCES = new Set(['request', 'to_request', 'fork', 'at', 'cut', 'after', 'last_seq', 'opened_by']);
/** A fork's trigger that names a line by its `seq` after its word (`open:102`, `trouble:139`): read the same way. */
const TRIGGER_AT = /^([a-z_]+):(\d+)$/;
/** Fields that name the build that ran, not what it wrote. */
const BUILD = new Set(['build_commit', 'build_dirty']);
/** Fields that are wall time on the machine that ran, as `t` is: a gate's call latency (#862), and how long a trunk
 * request waited for the forks in flight (`trunk.waited`, #782). */
const CLOCK = new Set(['latency_ms', 'wait_ms']);

type Line = Readonly<Record<string, unknown>> & { readonly seq: number; readonly kind: string };

/** Which lane each line is in, by its `seq`: the session's own (asks, settlements, rulings, meditations), the trunk's
 * requests and what answers them, each background job's end (it races the turn it ran in), and each fork's -- named by its lane, turn, why and question, and its place among
 * forks so named. */
export function lanesOf(lines: readonly LogLine[]): Map<number, string> {
  const all = lines as unknown as readonly Line[];
  const bySeq = new Map(all.map((line) => [line.seq, line]));
  const forks = new Map<number, string>();
  const named = new Map<string, number>();
  const laneOf = new Map<number, string>();
  const requestLane = (request: Line | undefined): string =>
    request && typeof request['fork'] === 'number' ? (forks.get(request['fork']) ?? 'trunk') : 'trunk';
  for (const line of all) {
    let lane = 'session';
    if (line.kind === 'fork') {
      const base = `fork:${String(line['lane'])}|${String(line['of_turn'])}|${String(line['why'])}|${String(line['question'] ?? '')}`;
      const n = named.get(base) ?? 0;
      named.set(base, n + 1);
      lane = `${base}#${n}`;
      forks.set(line.seq, lane);
    } else if (line.kind === 'background.ended') lane = `background:${String(line['job'])}`;
    else if (line.kind === 'request') lane = requestLane(line);
    else if (typeof line['fork'] === 'number' && forks.has(line['fork'])) lane = forks.get(line['fork'])!;
    else if (typeof line['to_request'] === 'number') lane = requestLane(bySeq.get(line['to_request']));
    else if (typeof line['request'] === 'number' && bySeq.get(line['request'])?.kind === 'request') lane = requestLane(bySeq.get(line['request']));
    laneOf.set(line.seq, lane);
  }
  return laneOf;
}

/** Each lane's lines, in order, as compared: `seq`, `t`, `opened`, a gate's latency and the build left out, and each reference read as
 * its target's lane and place in it. Everything else is compared, sizes and digests among it: a scenario shows its
 * model the same bytes on any machine (#859), so they are the same every run. */
export function byLane(lines: readonly LogLine[]): Record<string, readonly unknown[]> {
  const all = lines as unknown as readonly Line[];
  const laneOf = lanesOf(lines);
  const place = new Map<number, number>();
  const counts = new Map<string, number>();
  for (const line of all) {
    const lane = laneOf.get(line.seq)!;
    place.set(line.seq, counts.get(lane) ?? 0);
    counts.set(lane, (counts.get(lane) ?? 0) + 1);
  }
  const lanes: Record<string, unknown[]> = {};
  for (const line of all) {
    const compared: Record<string, unknown> = {};
    for (const [key, value] of Object.entries(line)) {
      if (key === 'seq' || key === 't' || key === 'opened' || BUILD.has(key) || CLOCK.has(key)) continue;
      const named = key === 'trigger' && typeof value === 'string' ? TRIGGER_AT.exec(value) : null;
      const target = named ? Number(named[2]) : undefined;
      compared[key] =
        REFERENCES.has(key) && typeof value === 'number' && laneOf.has(value)
          ? `${laneOf.get(value)!}#${place.get(value)!}`
          : named && target !== undefined && laneOf.has(target)
            ? `${named[1]}:${laneOf.get(target)!}#${place.get(target)!}`
            : value;
    }
    (lanes[laneOf.get(line.seq)!] ??= []).push(compared);
  }
  return lanes;
}

/** Each line's place: its lane, and its index among that lane's lines -- what `byLane` compares a line by. */
function placesOf(lines: readonly LogLine[]): Map<number, string> {
  const laneOf = lanesOf(lines);
  const counts = new Map<string, number>();
  const places = new Map<number, string>();
  for (const line of lines as unknown as readonly Line[]) {
    const lane = laneOf.get(line.seq)!;
    const n = counts.get(lane) ?? 0;
    counts.set(lane, n + 1);
    places.set(line.seq, `${lane}#${n}`);
  }
  return places;
}

/** Each line of `written` with its wall-clock fields (`CLOCK`) carried from the line at the same lane and place in
 * `previous`, the published example it replaces, when that line is of the same kind and holds the field: wall time
 * differs every run and is never compared, so carried, a rewrite changes only what the session changed. A field with
 * no such line keeps this run's value. How many were carried and how many kept their own. */
export function clockCarried(
  previous: readonly LogLine[],
  written: readonly LogLine[],
): { readonly log: LogLine[]; readonly carried: number; readonly own: number } {
  const before = placesOf(previous);
  const at = new Map<string, Line>();
  for (const line of previous as unknown as readonly Line[]) at.set(before.get(line.seq)!, line);
  const places = placesOf(written);
  let carried = 0;
  let own = 0;
  const log = (written as unknown as readonly Line[]).map((line) => {
    const keys = Object.keys(line).filter((key) => CLOCK.has(key));
    if (keys.length === 0) return line as unknown as LogLine;
    const was = at.get(places.get(line.seq)!);
    const out: Record<string, unknown> = { ...line };
    for (const key of keys) {
      if (was !== undefined && was.kind === line.kind && key in was) {
        out[key] = was[key];
        carried += 1;
      } else own += 1;
    }
    return out as unknown as LogLine;
  });
  return { log, carried, own };
}

/** The fields two lines of the same kind hold differently, by name; none when either is missing or the kinds differ. */
function fieldsApart(a: unknown, b: unknown): string[] {
  if (typeof a !== 'object' || a === null || typeof b !== 'object' || b === null) return [];
  const x = a as Record<string, unknown>;
  const y = b as Record<string, unknown>;
  if (x['kind'] !== y['kind']) return [];
  return [...new Set([...Object.keys(x), ...Object.keys(y)])].filter((key) => JSON.stringify(x[key]) !== JSON.stringify(y[key])).sort();
}

/** Where the published log and the one the scenario wrote today differ: per lane, its first differing line, or a lane
 * one has and the other lacks. Empty when they agree. */
export function laneDiff(published: readonly LogLine[], written: readonly LogLine[]): string[] {
  const a = byLane(published);
  const b = byLane(written);
  const said = (line: unknown) => (line === undefined ? 'nothing' : JSON.stringify(line));
  const kindOf = (line: unknown) => (line as { kind?: string } | undefined)?.kind ?? 'nothing';
  const out: string[] = [];
  for (const lane of [...new Set([...Object.keys(a), ...Object.keys(b)])].sort()) {
    const x = a[lane] ?? [];
    const y = b[lane] ?? [];
    for (let i = 0; i < Math.max(x.length, y.length); i++) {
      if (JSON.stringify(x[i]) === JSON.stringify(y[i])) continue;
      const fields = fieldsApart(x[i], y[i]);
      const where = fields.length > 0 ? ` -- ${fields.join(', ')} differ` : '';
      out.push(`${lane}, line ${i} (${kindOf(x[i])} published, ${kindOf(y[i])} written)${where}: published ${said(x[i])}, written ${said(y[i])}`);
      break;
    }
  }
  return out;
}

/** The images a log names by digest, each once, in the order first named: what an example publishes beside it, since
 * the page draws them. Never the outputs saved by digest: those hold the run's paths, and the page does not fetch them. */
export function imagesOf(lines: readonly LogLine[]): string[] {
  const named: string[] = [];
  for (const line of lines as unknown as readonly Line[]) {
    const files = line['files'];
    if (!Array.isArray(files)) continue;
    for (const file of files as readonly { readonly sha256?: unknown; readonly media_type?: unknown }[]) {
      if (typeof file.sha256 === 'string' && typeof file.media_type === 'string' && file.media_type.startsWith('image/') && !named.includes(file.sha256)) named.push(file.sha256);
    }
  }
  return named;
}

/** The `Declared-clean:` lines a regenerated example keeps from the one it replaces: a person's declaration that an
 * image is clean (`scripts/admission.py`), for each image the new log still names. Never written by the script. */
export function declarationsKept(migration: readonly string[], images: readonly string[]): string[] {
  return migration.filter((line) => {
    const declared = /^Declared-clean: ([0-9a-f]{64}) by /.exec(line);
    return declared !== null && images.includes(declared[1]!);
  });
}

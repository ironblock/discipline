/**
 * The native shell's notifications (#1049): a moment the session waits on the operator, told once as it begins, so
 * an operator whose window is behind another learns of it. Nothing here is new state: each moment is one the page
 * already shows -- a command waiting on approval (`/events`' `waiting`), a meditation held (#150's gate), the
 * model's phase proposal (#651). The shell sends the notice only while its window is not focused (`notify`, granted
 * to the `main` window alone); in a browser there is no shell, and nothing is sent.
 */

import type { Session } from '../session/fold.ts';
import type { Prompt } from './transport.ts';

/** What the session waits on the operator for, now. */
export interface Waits {
  readonly waiting?: Prompt | undefined;
  readonly held?: Session['held'];
  readonly proposal?: Session['proposal'];
}

/** One notification: its title, and one line under it. */
export interface Notice {
  readonly title: string;
  readonly body: string;
}

/** The most of a line a notification shows. */
const LINE = 120;

/** `text`'s first line, cut to what a notification shows. */
function line(text: string): string {
  const first = text.split('\n')[0] ?? '';
  return first.length > LINE || first !== text ? `${first.slice(0, LINE - 1)}…` : first;
}

/** The moments that begin between `was` and `now`: each wait `now` has that `was` did not. */
export function notices(was: Waits, now: Waits): Notice[] {
  const told: Notice[] = [];
  if (now.waiting && (now.waiting.request !== was.waiting?.request || now.waiting.id !== was.waiting.id)) {
    told.push({ title: 'a command waits on you', body: line(now.waiting.command) });
  }
  if (now.held && now.held.atTurn !== was.held?.atTurn) {
    told.push({ title: 'the meditation is held', body: 'the reconcile left your changes unaddressed' });
  }
  if (now.proposal && now.proposal.call !== was.proposal?.call) {
    const { from, to } = now.proposal;
    told.push({ title: 'the model proposes a phase move', body: from === undefined ? `to ${to}` : `${from} to ${to}` });
  }
  return told;
}

/** What the native shell exposes to its page: Tauri's IPC. Absent in a browser. */
interface Shell {
  readonly __TAURI_INTERNALS__?: { readonly invoke: (command: string, args?: unknown) => Promise<unknown> };
}

/** Ask the shell to tell `notice`; whether there was a shell to ask. */
export async function shellNotify(notice: Notice, scope: Shell = globalThis as Shell): Promise<boolean> {
  const invoke = scope.__TAURI_INTERNALS__?.invoke;
  if (!invoke) return false;
  await invoke('notify', { title: notice.title, body: notice.body });
  return true;
}

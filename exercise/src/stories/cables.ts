/**
 * A side call's cable, however connectors are drawn: in its own cell (as a
 * sweep), or as a pin on a trace net over the stage (Wiring.tsx) -- and
 * whether it waits for its slot, or carries light because it runs.
 */
export function cableOf(root: Element, id: string): { readonly pending: boolean; readonly live: boolean } | undefined {
  const swept = root.querySelector(`.ex-branchcell[data-branch="${CSS.escape(id)}"] .ex-cable`);
  if (swept) return { pending: swept.hasAttribute('data-pending'), live: swept.hasAttribute('data-live') };
  const net = [...root.querySelectorAll('.ex-wiring [data-net]')].find((g) => (g.getAttribute('data-to') ?? '').split(' ').includes(id));
  if (!net) return undefined;
  return { pending: net.hasAttribute('data-pending'), live: root.querySelector(`.ex-wiring [data-live][data-from="${CSS.escape(id)}"]`) !== null };
}

/**
 * The native shell's own page while it has no drive (the app audit, finding 6): it opens the window at once on
 * `?shell=starting`, then on the drive's page; a start that fails, or a sidecar that exits, sends it to
 * `?shell=failed&why=…`. A browser never sees either, and a page with a drive is none of these.
 */

export type ShellPage = { readonly kind: 'starting' } | { readonly kind: 'failed'; readonly why: string };

/** What the shell's page says, from the page's query; none on any other page. */
export function shellPage(search: string): ShellPage | undefined {
  const params = new URLSearchParams(search);
  switch (params.get('shell')) {
    case 'starting':
      return { kind: 'starting' };
    case 'failed':
      return { kind: 'failed', why: params.get('why') ?? 'the shell could not start its drive' };
    default:
      return undefined;
  }
}

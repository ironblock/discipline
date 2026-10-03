/**
 * What the replay page publishes (#32): recordings that happened, recorded
 * whole and migrated. The line is "no event whose content was authored after
 * the session" (#32, ruling 2): a fixture may carry authored content as a
 * development stand-in; a publication presents a session, and authored
 * content is not the session.
 */
export const PUBLISHED = ['first-drive', 'cancelled-capture', 'step-limit'] as const;

export type PublishedName = (typeof PUBLISHED)[number];

/**
 * Authored examples, published apart from the recordings and never as
 * sessions (the maintainer's ruling on #32, 2026-10-02; #272): each is listed
 * under its own heading and replayed under EXAMPLE_LABEL, which stays on the
 * page for the whole replay. Each is committed beside its admission in
 * src/drive/examples/, written from its source by scripts/write-examples.mjs.
 */
export const EXAMPLES = ['kitchen-sink'] as const;

export type ExampleName = (typeof EXAMPLES)[number];

/** The maintainer's sentence, verbatim: an example's label, and its admission's `Authored:` line. */
export const EXAMPLE_LABEL = 'this is an example of everything working the way we think it should, not a real session';

/** What is not published, and why: said on the page, not left to be wondered at. */
export const NOT_PUBLISHED: Readonly<Record<string, string>> = {
  'voxel-stress':
    'its side calls were written by a model after the session (scripts/stitch-sides.py), and the recording carries no per-event mark the page could draw to say which',
  specimen: 'authored, not recorded: the definition of done walked as a script',
};

export const isPublished = (name: string | null): name is PublishedName => name !== null && (PUBLISHED as readonly string[]).includes(name);

export const isExample = (name: string | null): name is ExampleName => name !== null && (EXAMPLES as readonly string[]).includes(name);

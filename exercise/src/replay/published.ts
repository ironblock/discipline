/**
 * What the replay page publishes (#32): recordings that happened, recorded
 * whole and migrated. The line is "no event whose content was authored after
 * the session" (#32, ruling 2): a fixture may carry authored content as a
 * development stand-in; a publication presents a session, and authored
 * content is not the session.
 */
export const PUBLISHED = ['first-drive', 'cancelled-capture', 'step-limit'] as const;

export type PublishedName = (typeof PUBLISHED)[number];

/** What is not published, and why: said on the page, not left to be wondered at. */
export const NOT_PUBLISHED: Readonly<Record<string, string>> = {
  'voxel-stress':
    'its side calls were written by a model after the session (scripts/stitch-sides.py), and the recording carries no per-event mark the page could draw to say which',
  'kitchen-sink': 'authored, not recorded: the cadence a working drive should have',
  specimen: 'authored, not recorded: the definition of done walked as a script',
};

export const isPublished = (name: string | null): name is PublishedName => name !== null && (PUBLISHED as readonly string[]).includes(name);

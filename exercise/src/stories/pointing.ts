import { userEvent, waitFor } from 'storybook/test';

/**
 * Point at `el` until `holds` passes, pointing again while it waits. A
 * story's pointer is synthetic, but Chromium's own mouse is there too,
 * unmoving, and reports a pointerover on whatever the page moves beneath it;
 * the surface rightly takes that as the pointer having moved, and what was
 * lit goes out. A slow runner put one between a story's hover and its check
 * (CI on #196). Pointing again is what a person's resting pointer amounts to,
 * so everything a story asserts while pointing goes in `holds`.
 */
export async function pointAt(el: Element, holds: () => Promise<void> | void): Promise<void> {
  await waitFor(async () => {
    await userEvent.hover(el);
    await holds();
  });
}

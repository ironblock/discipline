# Logo dark-mode experiment

Flip your OS / GitHub theme between light and dark; each logo should stay legible.
Grey background strips are not used on purpose: the page background is the test.

A. CSS `@media (prefers-color-scheme: dark)` inside the SVG, via `![]()`:

![A](css.svg)

B. `feColorMatrix` invert filter, enabled by the same media query, via `![]()`:

![B](filter.svg)

C. Two files with `<picture>` (GitHub's documented approach):

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="dark.svg">
  <img alt="C" src="light.svg">
</picture>

D. Two files selected by GitHub's own CSS via URL fragment (deprecated syntax; may not work):

![D dark](dark.svg#gh-dark-mode-only)
![D light](light.svg#gh-light-mode-only)

Mismatch test: set GitHub to light (Settings > Appearance) with the OS in dark, then reverse.
Exactly one logo should be visible per row A-D if it follows GitHub's theme.

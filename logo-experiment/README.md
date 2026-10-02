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

E. Does an `<img>` SVG blend with the page behind it? Three bars: a plain red one, a red one with
`mix-blend-mode: difference`, and a white one with `difference`. If the image is isolated (Chromium in
a local test) all three ignore the page: red, red, and white (invisible on a light page). If it blends
with the page (WebKitGTK in a local test) the second turns cyan and the third black on a light page.
Look at it in light and dark GitHub themes, on your phone:

![E](blend.svg)

MDN's `invert()` example puts the filter on the host page: CSS `filter` on an `<img>`, with the SVG
filter defined in the page's own markup. That only helps if GitHub lets README markdown carry it. Each
row below shows a red bar; if the markup survives GitHub's sanitiser the bar is cyan, if it is stripped
the bar stays red.

F. `style="filter: invert(1)"` on the `<img>`:

<img src="bar.svg" style="filter: invert(1)" alt="F">

G. An inline `<svg>` filter (`feComponentTransfer`, as in MDN's example) referenced from the `<img>`:

<svg height="0"><filter id="inv"><feComponentTransfer><feFuncR type="table" tableValues="1 0"/><feFuncG type="table" tableValues="1 0"/><feFuncB type="table" tableValues="1 0"/></feComponentTransfer></filter></svg>
<img src="bar.svg" style="filter: url(#inv)" alt="G">

H. A `<style>` block with a dark-scheme media query:

<style>@media (prefers-color-scheme: dark) { .h { filter: invert(1) } }</style>
<img class="h" src="bar.svg" alt="H">


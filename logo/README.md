# logo

`logo-dark.svg` and `logo-light.svg` are generated. Edit `build.py` and `lens.py`, not the SVGs:

    pip install fonttools numpy scipy pillow
    python3 logo/build.py            # the default variant -> logo/logo-{dark,light}.svg
    python3 logo/build.py --options  # every variant -> logo/options/ (not committed) with a README to view them
    python3 logo/playground.py       # logo/playground.html (not committed): every setting as a control

The build fails if a filter input or `url(#id)` refers to something that does not exist, or if
an image loads from anywhere but a `data:` URI. Browsers ignore both silently.

Typeface: Barlow Black (`font/`, SIL OFL 1.1, license text alongside). Both files are byte-identical
to the npm package `@fontsource/barlow` 5.3.0's `files/barlow-latin-900-normal.woff`
(sha256 `8783b44e955c5eae3e236050357ca3ddc7640c96156ca1dede6ce9cfe88bd99e`) and its `LICENSE`. The
word is outlined into paths because GitHub renders README SVGs through `<img>`, which cannot load
fonts, so the SVGs embed no font.

Serve the pair with `<picture>` (the root `README.md` does): on GitHub the `prefers-color-scheme`
source follows GitHub's theme setting, while a media query inside an `<img>` SVG follows the OS.

## The design

Three sine waves (R, G, B, a third of a turn apart) enter on the left and leave the `e` as one beam.
Their peak-to-peak height is the x-height times the variant's `amp`, and the wavelength is long
and lazy (`WAVELENGTH`). Harmonics ("noise") ride on the waves and are stripped letter by letter. The
amplitude falls to zero along the word as `(1 - t^a)^b` (`TAPERS`): `late` is `1 - t^2` and `early` is
`(1 - t)^2`; the arithmetic mean of the two is exactly linear, so `mid` splits the difference in the
exponents instead. Between the first and last letter the light shows only inside the glass ("story");
outside, the waves and the beam run on. Red and green cross `cross` px left of the D.

The default variant is `tuned`, set by hand in the playground: displacement-filter glass
(`glass="refract"`), the `tuned` look in `lens.LOOKS`, late taper, noise harmonics, story clipping.

### Palette

Layered, in `build.py`: `PALETTE` is what both themes share, `DELTA["dark"]` and `DELTA["light"]` are what
each changes, and `layered()` merges them (`palette(theme, variant)`). `THEMES` is the older, complete
pair, used by the retired variants only. Light mode has tuned colours; its other values were
approximated from the dark ones by ratio and by eye (`ce1b5f1`), not tuned.

## How the glass works

`lens.py` follows https://kube.io/blog/liquid-glass-css-svg/ : a convex bezel profile, Snell's law at
the top surface, displacement along the edge normal, with distance and normal taken from a distance
transform of the rasterised glyphs (that post has a closed form for a rounded rectangle). A look in
`LOOKS` is bezel width, glass thickness, refractive index and how far the rim light spreads.

- **`glass="refract"`** (default): the field is baked into one PNG that a `bend` filter reads with
  `feImage` and applies with `feDisplacementMap` (two primitives) to each layer of light. The lines
  stay plain, unbent paths, so something can move them later. The rim light is outline strokes, cut
  into segments whose brightness follows the edge normal.
- **`glass="elements"`**: the same field is applied to the *geometry* at build time. No `feImage`, no
  displacement filter, only masks, clips and plain blurs. The bend is exaggerated (`REFRACT_GAIN`)
  because thin strokes barely show the true shift, and content is not mirrored at the rim the way the
  real filter does. The fallback if the filter misbehaves somewhere.
- **`glass="filter"`**: the older all-filter version, with a second PNG for the rim light.

## Playground

`playground.template.html` plus `playground.py` make one page. Its script ports `build.py`'s `svg()` for
`glass="refract"` and also `lens.py`'s refraction map (canvas raster, distance transform, bezel
profile, Snell's law), so bezel width, thickness, refractive index and the surface profile are controls.
The profiles (convex squircle, convex circle, concave, lip) are kube.io's; the article writes concave
and lip in terms of "Convex", and the squircle is assumed. Each preset opens exactly as `build.py`
renders it. Settings are per palette (the Dark/Light toggle chooses which) or shared. "Copy settings"
gives the changes from a preset, to be written into `PALETTE`, `DELTA` and `VARIANTS`.

Against `build.py` the page's logos differ by ~0.3/255 on average in Chromium (it rasterises with
canvas, `lens.py` with PIL, and the gain magnifies the difference). Nothing gates that: the page and
`build.py`'s `svg()` are two implementations of one generator.

## What was found

Checked in Chromium and WebKitGTK 2.52 (not a stand-in for Safari: see below), and on an iPhone through
GitHub or through the playground, as noted.

- **Dark and light.** `<picture>` with `prefers-color-scheme` follows GitHub's theme; a media query
  inside an `<img>` SVG follows the OS, so system-dark with GitHub-light shows the wrong one.
- **One image that reads the background: not possible in a README.** A filter inside an `<img>` SVG
  sees only the SVG's own pixels, `feComponentTransfer` included. `BackgroundImage` as a filter input
  draws the same on a dark and a light page in both engines. `currentColor` is black. `mix-blend-mode`
  is isolated from the page in Chromium and on the iPhone through GitHub (no colour change), but WebKitGTK
  blends with the page, so WebKitGTK is not a reliable proxy for compositing. Host-page filters, the
  MDN `invert()` example's way, need CSS or an inline `<svg>` in the page; GitHub's README did not apply
  `style="filter: ..."`, an inline `<svg>` filter or a `<style>` block (iPhone, GitHub). Tests:
  `git show 26779ca:logo-experiment/README.md`.
- **Safari and filters.** A 104-primitive glass filter painted nothing in iOS Safari (only the body tint
  showed). It reproduced in WebKitGTK and was narrowed to a stair-shaped refraction map; padding a
  working filter to 104 primitives did not break it, so it is not a plain count and the mechanism is
  unknown. `MAX_PRIMITIVES` in `build.py` is a heuristic ceiling; the shipped pair's filters have 1 to 4 primitives each.
  `feImage` with a `data:` PNG renders on an iPhone through GitHub (seen for earlier builds; the
  current default was tuned on an iPhone through the playground's `<img>`, not through GitHub).
- **Resolution.** In WebKitGTK at 3x the filter output is blocky (below device resolution), hidden in the
  final logo by the blur. Not seen on a device.
- **Not seen:** the current default through GitHub in light mode, Mac Safari, Firefox.

## Lineage

The branch `logo-dark-mode-experiment` keeps the whole history. The retired option SVGs and the GitHub
test pages were deleted from the tree at the end; to see them, `git show 6f3c958:logo/options/README.md`
(the picture pairs; `git ls-tree -r 6f3c958 logo/options` lists the files) and
`git show 26779ca:logo-experiment/README.md`.

| Commits | What |
| --- | --- |
| `fbbe54a` `a44525a` | Does GitHub follow its own theme or the OS? Four ways to serve a logo |
| `389ace2` | First glass logo, D-DIN Exp Bold (SIL OFL 1.1, its licence committed with it; replaced by Barlow Black in `1e35a3f`) |
| `1e35a3f` `6cada60` | Barlow Black, pastel waves, frosted glass, inner bloom |
| `93b03f3` | Smaller glass filter after iOS Safari painted nothing |
| `fc5d801` | Refraction and rim light baked into `feImage` maps, after kube.io |
| `07603f4` `d5c6977` | x-height waves, quadratic taper, glass built from elements |
| `0ae06fb` `c05ffda` `a01a6e7` | Story clipping, softer chamfer, noise plus story |
| `920e5e4` `8d6c6ef` | Red/green crossover, placed outside the D's edge |
| `48c7a62` `63866a3` `12c7c42` | Waves bent by one displacement filter; the playground and its in-page map |
| `d93325a` `ce1b5f1` `6f3c958` | `tuned` as default; light approximated, then its colours tuned |
| `317a177` `26779ca` | Tests of a background-aware single image |

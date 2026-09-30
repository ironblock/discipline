# logo

`logo-dark.svg` and `logo-light.svg` are generated. Edit `build.py` and `lens.py`, not the SVGs:

    pip install fonttools numpy scipy pillow
    python3 logo/build.py            # the default variant -> logo/logo-{dark,light}.svg
    python3 logo/build.py --options  # every variant -> logo/options/ (with a README to view them)

The build fails if a filter input or `url(#id)` refers to something that does not exist, or if
an image loads from anywhere but a `data:` URI. Browsers ignore both silently.

Typeface: Barlow Black (`font/`, SIL OFL 1.1, license text alongside). The word is outlined
into paths because GitHub renders README SVGs through `<img>`, which cannot load fonts.

## How the glass works

`lens.py` bakes two PNGs from the letter outlines and the filter reads them with `feImage`,
following https://kube.io/blog/liquid-glass-css-svg/ : a convex bezel profile, Snell's law at
the top surface, displacement along the edge normal. Where that post has a closed-form distance
for a rounded rectangle, this uses a distance transform of the rasterised glyphs. One map
drives `feDisplacementMap`; the other carries the rim light (top-left, bottom-right) and an
edge band. `LOOKS` in `lens.py` sets bezel width and rim spread: narrow reads as carved, wide as
liquid. Bloom, frost and the light itself are plain filter primitives.

## Safari

An earlier 104-primitive glass filter painted nothing in iOS Safari (only the body tint showed).
It reproduced in WebKitGTK 2.52 and was narrowed to a stair-shaped refraction map; padding a
working filter to 104 primitives did not break it, so it is not a plain count and the mechanism
is unknown. The filter is now ~24 primitives, and `MAX_PRIMITIVES` in `build.py` is a heuristic
ceiling, not a known limit.

Checked in WebKitGTK 2.52 (same WebCore as Safari) against Chromium at 1x, 2x and 3x: mean pixel
difference ~3/255 in dark, ~6/255 in light. **Not verified on an iPhone or Mac Safari, and not on
GitHub itself.** `options/README.md` has an A/B page for exactly that.

Serve the pair with `<picture>`, not a media query inside the SVG: on GitHub the
`<picture>` `prefers-color-scheme` source follows GitHub's theme setting, while a media
query inside an `<img>` SVG follows the OS.

# logo

`logo-dark.svg` and `logo-light.svg` are generated. Edit `build.py` and `lens.py`, not the SVGs:

    pip install fonttools numpy scipy pillow
    python3 logo/build.py            # the default variant -> logo/logo-{dark,light}.svg
    python3 logo/build.py --options  # every variant -> logo/options/ (with a README to view them)

The build fails if a filter input or `url(#id)` refers to something that does not exist, or if
an image loads from anywhere but a `data:` URI. Browsers ignore both silently.

Typeface: Barlow Black (`font/`, SIL OFL 1.1, license text alongside). The word is outlined
into paths because GitHub renders README SVGs through `<img>`, which cannot load fonts.

## The light

Three sine waves (R, G, B, a third of a turn apart) enter on the left and leave the `e` as one beam.
Their peak-to-peak height is the font's x-height, so they stay inside the lowercase band, and the
wavelength is long and lazy (`WAVELENGTH`). The amplitude falls to zero along the word by a quadratic
easing (`taper` in `build.py`): ease-in stays lively and settles late (default), ease-out calms early.

## How the glass works

Two ways to make it, both from `lens.py`, which follows https://kube.io/blog/liquid-glass-css-svg/ :
a convex bezel profile, Snell's law at the top surface, displacement along the edge normal, with
distance and normal taken from a distance transform of the rasterised glyphs (that post has a
closed form for a rounded rectangle). `LOOKS` sets bezel width and rim spread: narrow reads as
carved, wide as liquid.

- **`glass="filter"`**: the field is baked into two PNGs (refraction, rim light) that a filter reads
  with `feImage` and applies with `feDisplacementMap`.
- **`glass="elements"`** (default): the same field is applied to the *geometry* at build time. The wave
  paths are bent where they cross each bezel and drawn as ordinary paths, the glow is layered copies of
  them with plain blurs, and the rim light is the letter outline stroked in segments whose brightness
  follows the edge normal. No `feImage`, no displacement filter. The bend is exaggerated
  (`REFRACT_GAIN`) because thin strokes barely show the true shift, and content is not mirrored at the rim
  the way the real filter does.

## Safari

An earlier 104-primitive glass filter painted nothing in iOS Safari (only the body tint showed).
It reproduced in WebKitGTK 2.52 and was narrowed to a stair-shaped refraction map; padding a
working filter to 104 primitives did not break it, so it is not a plain count and the mechanism
is unknown. The filter glass is now ~24 primitives, and `MAX_PRIMITIVES` in `build.py` is a heuristic
ceiling, not a known limit.

Checked in WebKitGTK 2.52 (same WebCore as Safari) against Chromium at 1x, 2x and 3x: mean pixel
difference ~3/255 in dark, ~6/255 in light. **Not verified on an iPhone or Mac Safari, and not on
GitHub itself.** `options/README.md` has a page and a small diagnostic for exactly that. `elements` is the
default because it uses none of what has failed or is unproven on a device.

Serve the pair with `<picture>`, not a media query inside the SVG: on GitHub the
`<picture>` `prefers-color-scheme` source follows GitHub's theme setting, while a media
query inside an `<img>` SVG follows the OS.

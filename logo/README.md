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
wavelength is long and lazy (`WAVELENGTH`). The amplitude falls to zero along the word as
`(1 - t^a)^b` (`TAPERS` in `build.py`): `late` is `1 - t^2`, `early` is `(1 - t)^2`, and `mid` (the
default) is `a = b = 1.5`. The arithmetic mean of `late` and `early` is exactly linear, so `mid`
splits the difference in the exponents instead.

Options (`build.py --options`, viewable in `options/README.md`): `elements` (default), `early`, `late`,
`noise` (harmonics the glass strips letter by letter), `story` (the incoming waves and outgoing beam are
drawn, but between the first and last letter the light shows only inside the glass), `story-noise` (story's clipping with noise's tangle), `inside` (nothing outside
the letters at all, canvas cropped to the word), `filter-glass` (the displacement-filter glass, kept as a
fallback).

## How the glass works

Two ways to make it, both from `lens.py`, which follows https://kube.io/blog/liquid-glass-css-svg/ :
a convex bezel profile, Snell's law at the top surface, displacement along the edge normal, with
distance and normal taken from a distance transform of the rasterised glyphs (that post has a
closed form for a rounded rectangle). `LOOKS` sets bezel width and rim spread: narrow reads as
carved, wide as liquid.

- **`glass="filter"`**: the field is baked into two PNGs (refraction, rim light) that a filter reads
  with `feImage` and applies with `feDisplacementMap`.
- **`glass="refract"`**: `elements`, except the waves are bent by a displacement filter instead of at
  build time: the lines stay plain, unbent paths (so something can move them later) and one `bend`
  filter (`feImage` + `feDisplacementMap`, 2 primitives) applied to each light layer reads the same
  baked refraction map. The rim light is still outline strokes. In WebKitGTK the displacement
  matches Chromium, but at 3x the filter output is blocky (see Safari).
- **`glass="elements"`** (default): the same field is applied to the *geometry* at build time. The wave
  paths are bent where they cross each bezel and drawn as ordinary paths, the glow is layered copies of
  them with plain blurs, and the rim light is the letter outline stroked in segments whose brightness
  follows the edge normal. No `feImage`, no displacement filter. The bend is exaggerated
  (`REFRACT_GAIN`) because thin strokes barely show the true shift, and content is not mirrored at the rim
  the way the real filter does.

## Safari

An earlier 104-primitive glass filter painted nothing in iOS Safari (only the body tint showed). It
reproduced in WebKitGTK 2.52 and was narrowed to a stair-shaped refraction map; padding a working
filter to 104 primitives did not break it, so it is not a plain count and the mechanism is unknown.
The filter glass is now ~24 primitives, and `MAX_PRIMITIVES` in `build.py` is a heuristic ceiling.

A screenshot from an iPhone, through GitHub in dark mode, shows both glass types rendering: the
`feImage` one and `elements`. Not seen: light mode on a device, Mac Safari, Firefox. In WebKitGTK at 3x
the filter glass renders blocky (filter output below device resolution) and `elements` does not; real
Safari may differ. Against Chromium in WebKitGTK the mean pixel difference is ~1-3/255 in dark and
~2-5/255 in light.

`elements` is the default because it avoids the displacement filter and the large filter graph, and
needs only masks, clips and plain blurs.

Serve the pair with `<picture>`, not a media query inside the SVG: on GitHub the
`<picture>` `prefers-color-scheme` source follows GitHub's theme setting, while a media
query inside an `<img>` SVG follows the OS.

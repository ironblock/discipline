# logo

`logo-dark.svg` and `logo-light.svg` are generated. Edit `build.py`, not the SVGs:

    pip install fonttools brotli   # brotli only needed for woff2 fonts
    python3 logo/build.py            # the default variant -> logo/logo-{dark,light}.svg
    python3 logo/build.py --options  # every variant -> logo/options/ (with a README to view them)

The build fails if a filter input or `url(#id)` refers to something that does not exist;
browsers silently ignore those, so they would otherwise show up only as a visual glitch.

Typeface: Barlow Black (`font/`, SIL OFL 1.1, license text alongside). The word is
outlined into paths because GitHub renders README SVGs through `<img>`, which cannot
load fonts.

Effects use only long-standing SVG filter primitives (no `mix-blend-mode`, `feImage`,
or lighting primitives), chosen for Safari. **Not yet verified in Safari or Firefox**;
only Chromium has been rendered.

Serve the pair with `<picture>`, not a media query inside the SVG: on GitHub the
`<picture>` `prefers-color-scheme` source follows GitHub's theme setting, while a media
query inside an `<img>` SVG follows the OS.

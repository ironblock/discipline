# logo

`logo-dark.svg` and `logo-light.svg` are generated. Edit `build.py`, not the SVGs:

    pip install fonttools brotli   # brotli only needed for woff2 fonts
    python3 logo/build.py

Typeface: Barlow Black (`font/`, SIL OFL 1.1, license text alongside). The word is
outlined into paths because GitHub renders README SVGs through `<img>`, which cannot
load fonts.

Effects use only long-standing SVG filter primitives (no `mix-blend-mode`, `feImage`,
or lighting primitives), chosen for Safari. **Not yet verified in Safari or Firefox**;
only Chromium has been rendered.

Serve the pair with `<picture>`, not a media query inside the SVG: on GitHub the
`<picture>` `prefers-color-scheme` source follows GitHub's theme setting, while a media
query inside an `<img>` SVG follows the OS.

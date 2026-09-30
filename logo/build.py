#!/usr/bin/env python3
"""Generate the Discipline logo as light/dark SVG pairs from one geometry.

Three phase-shifted sine waves (R, G, B) enter the left of the word, converge
as they pass through glass letter blocks, and leave the final "e" as one beam.

GitHub loads README SVGs through <img>: no web fonts, no external files, so the
word is outlined from the font and every effect is a self-contained SVG filter.

Safari: only long-standing filter primitives are used (feImage, feDisplacementMap,
feColorMatrix, feComposite, feGaussianBlur, feMorphology, feComponentTransfer, feFlood,
feMerge, feOffset). No mix-blend-mode, no lighting primitives. The refraction and rim
light are baked into two PNGs (lens.py) that the filter reads with feImage, as
kube.io's write-up does; every length is in user units. Checked in WebKitGTK 2.52, not
on a device; see logo/README.md.

usage: build.py [--font PATH] [outdir]            default logo-{dark,light}.svg
       build.py --options [outdir]                every variant + options/README.md
"""
import argparse
import math
import pathlib
import re
import sys
import xml.etree.ElementTree as ET

sys.path.insert(0, str(pathlib.Path(__file__).parent))

import lens
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont

HERE = pathlib.Path(__file__).parent
WORD = "Discipline"
DEFAULT_FONT = HERE / "font" / "barlow-latin-900-normal.woff"

WORD_WIDTH = 770  # px; font size follows from this
TRACKING = 0.02   # em, added after each letter
MARGIN = 215
H = 290
BASELINE = 218
AMPLITUDE = 56
WAVELENGTH = 120

# A 104-primitive glass filter (4-band stair refraction map) painted nothing in iOS Safari and in
# WebKitGTK 2.52. The same graph with 3 bands (88) painted, and padding a working graph to 104 with
# identity primitives also painted, so this is not a plain count and the mechanism is unknown.
# This ceiling is a heuristic: the working graphs are under 60. Re-check in WebKit if it grows.
MAX_PRIMITIVES = 64
CAST = (12, 14)   # px, where light spilled by the glass lands
PAD = 16          # px around the letters covered by the baked maps

# What each option changes. Keys not listed keep BASE_VARIANT's value.
BASE_VARIANT = dict(harmonics="single", cast=False, look="carved")
VARIANTS = {
    "bloom": dict(
        desc="Light trapped in the glass: bloom inside each letter, streaked along the direction of travel, "
             "with chamfers that pick up the beam's colour.",
    ),
    "filter": dict(
        desc="Noise to signal: each waveform carries harmonics that the glass strips away letter by letter, "
             "so the tangle on the left resolves into one line.",
        harmonics="filter",
    ),
    "caustic": dict(
        desc="Glass that spills light: a coloured caustic falls from the letters onto the page.",
        cast=True,
    ),
    "liquid": dict(
        desc="Same light, softer glass: a wide bezel that bends more of the beam and a rim light that spreads.",
        look="liquid",
    ),
    "pick": dict(
        desc="Noise to signal, with bloom and a faint spill. The combination I would ship.",
        harmonics="filter", cast=True,
    ),
}
DEFAULT_VARIANT = "bloom"

THEMES = {
    "dark": dict(
        wave=("#ffa3b3", "#a3ffc6", "#a9c0ff"), wave_alpha=0.60,
        beam="#ffffff", beam_width=6,
        glow_wide=9, glow_wide_a=1.0, glow_tight=3,
        body="#cfd8ff", body_top=0.26, body_bottom=0.12,
        lit="#ffffff", lit_opacity=0.75,
        shade="#8a7dff", shade_opacity=0.50,
        edge="#ffffff", edge_opacity=0.16,
        rim="#ffffff", rim_opacity=0.35,
        frost=1.8,
        inner=1.5,  # strokes are this much thicker inside the glass, as if focused
        beam_inner=1.5,
        # gain/alpha_gain: brighten the light before it is spread. a, b: near and far
        # blur (x, y); wider in x, along the travel direction. slopes: how much of each
        # survives. catch: how strongly the chamfers pick up colour. cast: spill strength.
        bloom=dict(gain=1.15, alpha_gain=1.9, a=(6, 3.5), a_slope=0.9, b=(26, 9), b_slope=2.1,
                   catch=2.8, cast=0.55),
    ),
    "light": dict(
        wave=("#e0182d", "#12b84a", "#2350e0"), wave_alpha=0.33,
        beam="#0b1020", beam_width=6,
        glow_wide=2.5, glow_wide_a=0.22, glow_tight=0.7,
        body="#55608a", body_top=0.10, body_bottom=0.22,
        lit="#ffffff", lit_opacity=0.95,
        shade="#1c2340", shade_opacity=0.60,
        edge="#1c2340", edge_opacity=0.14,
        rim="#1c2340", rim_opacity=0.55,
        frost=1.8,
        inner=1.5,
        beam_inner=0.9,  # a black beam's bloom reads as smoke, not glow
        bloom=dict(gain=1.0, alpha_gain=1.0, a=(5, 3), a_slope=0.6, b=(20, 7), b_slope=0.6,
                   catch=1.0, cast=0.4),
    ),
}


def layout(font_path):
    """Positions the word once. The SVG path and the glyph contours the maps are baked
    from both come from here."""
    font = TTFont(font_path)
    cmap, glyphs, hmtx = font.getBestCmap(), font.getGlyphSet(), font["hmtx"]
    upm = font["head"].unitsPerEm
    advances = [hmtx[cmap[ord(ch)]][0] / upm for ch in WORD]
    em = WORD_WIDTH / (sum(advances) + TRACKING * (len(WORD) - 1))
    scale = em / upm
    x = MARGIN
    parts, spans, contours = [], [], []
    for ch, adv in zip(WORD, advances):
        name = cmap[ord(ch)]
        pen = SVGPathPen(glyphs, ntos=lambda v: f"{v:.1f}".rstrip("0").rstrip("."))
        glyphs[name].draw(TransformPen(pen, (scale, 0, 0, -scale, x, BASELINE)))
        parts.append(pen.getCommands())
        contours += lens.glyph_contours(glyphs, name, x, BASELINE, scale)
        spans.append((x, x + adv * em))
        x += (adv + TRACKING) * em
    xs = [p[0] for c in contours for p in c]
    ys = [p[1] for c in contours for p in c]
    region = (math.floor(min(xs)) - PAD, math.floor(min(ys)) - PAD,
              math.ceil(max(xs)) - math.floor(min(xs)) + 2 * PAD, math.ceil(max(ys)) - math.floor(min(ys)) + 2 * PAD)
    x_height = font["OS/2"].sxHeight * scale
    return dict(d=" ".join(parts), spans=spans, width=MARGIN + WORD_WIDTH + MARGIN,
                beam_y=BASELINE - x_height / 2,
                maps={name: lens.maps(contours, region, look) for name, look in lens.LOOKS.items()})


def smoothstep(a, b, x):
    t = min(1.0, max(0.0, (x - a) / (b - a)))
    return t * t * (3 - 2 * t)


def convergence(spans):
    """x where amplitude starts to fall, and x where it has reached zero."""
    return spans[0][0] - 10, spans[-1][0] + 0.55 * (spans[-1][1] - spans[-1][0])


def harmonics(spans, mode):
    """(period, amplitude, fade start x, fade end x) for each component of a wave.
    In "filter" mode the shorter components are stripped early, one letter at a time."""
    start, end = convergence(spans)
    if mode == "single":
        return [(WAVELENGTH, AMPLITUDE, start, end)]
    return [
        (WAVELENGTH, 46, start, end),
        (62, 17, spans[0][0], spans[3][1]),
        (39, 8, spans[0][0], spans[1][1]),
    ]


def wave_paths(spans, width, beam_y, mode):
    comps = harmonics(spans, mode)
    step = 3 if mode == "single" else 2
    paths = []
    for k in range(3):
        pts, x = [], 0.0
        while x <= width + step:
            y = beam_y
            for h, (period, amp, fade0, fade1) in enumerate(comps):
                phase = k * 2 * math.pi / 3 * (1 + h) + 0.9 * h * k
                y += amp * (1 - smoothstep(fade0, fade1, x)) * math.sin(2 * math.pi * x / period + phase)
            pts.append(f"{x:.0f},{y:.1f}")
            x += step
        paths.append("M" + " L".join(pts))
    return paths


def rgb01(hexcolor):
    return [int(hexcolor[i:i + 2], 16) / 255 for i in (1, 3, 5)]


def from_channel(src, channel, color, opacity, result):
    """A layer of one solid colour whose alpha is one channel of a baked map, times opacity."""
    r, g, b = rgb01(color)
    alpha = ["0", "0", "0"]
    alpha["RGB".index(channel)] = f"{opacity:g}"
    return (f'<feColorMatrix in="{src}" type="matrix" '
            f'values="0 0 0 0 {r:.3f}  0 0 0 0 {g:.3f}  0 0 0 0 {b:.3f}  {alpha[0]} {alpha[1]} {alpha[2]} 0 0" result="{result}"/>')


def glass_filter(t, v, w, maps):
    b = t["bloom"]
    rx, ry, rw, rh = maps["region"]
    image = f'x="{rx}" y="{ry}" width="{rw}" height="{rh}" preserveAspectRatio="none"'
    p = []
    add = p.append
    add('<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 30 0" result="mask"/>')
    # The letter-shaped carrier fill that gives this filter its shape is far fainter than any light.
    add('<feComponentTransfer in="SourceGraphic" result="light"><feFuncA type="linear" slope="1.4" intercept="-0.1"/></feComponentTransfer>')

    add("<!-- refraction: baked from each letter's distance field and Snell's law (lens.py) -->")
    add(f'<feImage href="{maps["disp"]}" {image} result="dmap"/>')
    add(f'<feDisplacementMap in="light" in2="dmap" scale="{maps["scale"]:.2f}" xChannelSelector="R" yChannelSelector="G" result="bent"/>')

    add("<!-- frost, then bloom: brighten the light and spread it, further along x than y -->")
    add(f'<feGaussianBlur in="bent" stdDeviation="{t["frost"]}" result="frost"/>')
    g, ag = b["gain"], b["alpha_gain"]
    add(f'<feColorMatrix in="bent" type="matrix" values="{g} 0 0 0 0  0 {g} 0 0 0  0 0 {g} 0 0  0 0 0 {ag} 0" result="hot"/>')
    for name in ("a", "b"):
        add(f'<feGaussianBlur in="hot" stdDeviation="{b[name][0]} {b[name][1]}" result="bloom{name.upper()}0"/>')
        add(f'<feComponentTransfer in="bloom{name.upper()}0" result="bloom{name.upper()}"><feFuncA type="linear" slope="{b[name + "_slope"]}"/></feComponentTransfer>')

    add("<!-- rim light from the baked map: R lit from the top-left, G from the bottom-right, B edge band -->")
    add(f'<feImage href="{maps["spec"]}" {image} result="spec"/>')
    add(from_channel("spec", "R", t["lit"], t["lit_opacity"], "lit"))
    add(from_channel("spec", "G", t["shade"], t["shade_opacity"], "shade"))
    add(from_channel("spec", "B", t["edge"], t["edge_opacity"], "edge"))
    add('<feColorMatrix in="spec" type="matrix" values="0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 1 0 0" result="band"/>')
    add("<!-- near a beam, the edge takes its colour -->")
    add('<feComposite in="bloomA" in2="band" operator="in" result="catchRaw"/>')
    add(f'<feComponentTransfer in="catchRaw" result="catch"><feFuncA type="linear" slope="{b["catch"]}"/></feComponentTransfer>')

    add("<!-- hairline rim -->")
    add('<feMorphology in="mask" operator="erode" radius="0.9" result="inner"/>')
    add('<feComposite in="mask" in2="inner" operator="out" result="rimShape"/>')
    add(f'<feFlood flood-color="{t["rim"]}" flood-opacity="{t["rim_opacity"]}"/>')
    add('<feComposite in2="rimShape" operator="in" result="rim"/>')

    add('<feMerge result="inside">'
        '<feMergeNode in="bloomB"/><feMergeNode in="frost"/><feMergeNode in="bloomA"/><feMergeNode in="catch"/>'
        '<feMergeNode in="edge"/><feMergeNode in="shade"/><feMergeNode in="lit"/><feMergeNode in="rim"/></feMerge>')
    add('<feComposite in="inside" in2="mask" operator="in" result="insideClip"/>')

    if v["cast"]:
        add("<!-- light spilled by the glass lands on the page, down-right of the letters -->")
        add(f'<feOffset in="bloomB" dx="{CAST[0]}" dy="{CAST[1]}" result="castRaw"/>')
        add('<feComposite in="castRaw" in2="mask" operator="out" result="castOut"/>')
        add(f'<feComponentTransfer in="castOut" result="cast"><feFuncA type="linear" slope="{b["cast"]}"/></feComponentTransfer>')
        add('<feMerge><feMergeNode in="cast"/><feMergeNode in="insideClip"/></feMerge>')
    else:
        add('<feMerge><feMergeNode in="insideClip"/></feMerge>')

    body = "\n      ".join(p)
    return (f'<filter id="glass" filterUnits="userSpaceOnUse" x="0" y="0" width="{w}" height="{H}" '
            f'color-interpolation-filters="sRGB">\n      {body}\n    </filter>')


def svg(theme, variant, geo):
    t, v = THEMES[theme], {**BASE_VARIANT, **VARIANTS[variant]}
    word_d, spans, width, beam_y = geo["d"], geo["spans"], geo["width"], geo["beam_y"]
    start, end = convergence(spans)
    w = f"{width:.0f}"
    fade_in, fade_out = 110 / width, 1 - 150 / width
    ramp0, ramp1 = (end - 45) / width, (end + 25) / width
    wave_defs = "\n".join(
        f'    <path id="w{k}" d="{d}"/>' for k, d in enumerate(wave_paths(spans, width, beam_y, v["harmonics"]))
    )

    def light(scale, beam_scale):
        strokes = "\n".join(f'        <use href="#w{k}" stroke="{c}"/>' for k, c in enumerate(t["wave"]))
        return f"""      <g fill="none" stroke-width="{5 * scale:g}" stroke-linecap="round" stroke-linejoin="round" stroke-opacity="{t['wave_alpha']}" mask="url(#wavemask)">
{strokes}
      </g>
      <use href="#beam" fill="none" stroke="{t['beam']}" stroke-width="{t['beam_width'] * beam_scale:g}" stroke-linecap="round" mask="url(#beammask)"/>"""

    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{H}" viewBox="0 0 {w} {H}" role="img" aria-label="Discipline">
  <title>Discipline</title>
  <defs>
    <path id="word" d="{word_d}"/>
    <clipPath id="wordclip"><use href="#word"/></clipPath>
    <mask id="outside" maskUnits="userSpaceOnUse" x="0" y="0" width="{w}" height="{H}">
      <rect width="{w}" height="{H}" fill="#fff"/>
      <use href="#word" fill="#000"/>
    </mask>

    <linearGradient id="body" gradientUnits="userSpaceOnUse" x1="0" y1="{BASELINE - 130}" x2="0" y2="{BASELINE + 35}">
      <stop offset="0" stop-color="{t['body']}" stop-opacity="{t['body_top']}"/>
      <stop offset="1" stop-color="{t['body']}" stop-opacity="{t['body_bottom']}"/>
    </linearGradient>

    <!-- waves fade in from the left and hand over to the solid beam as they converge -->
    <linearGradient id="wavefade" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="{w}" y2="0">
      <stop offset="0" stop-color="#fff" stop-opacity="0"/>
      <stop offset="{fade_in:.4f}" stop-color="#fff"/>
      <stop offset="{ramp0:.4f}" stop-color="#fff"/>
      <stop offset="{ramp1:.4f}" stop-color="#fff" stop-opacity="0"/>
    </linearGradient>
    <linearGradient id="beamfade" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="{w}" y2="0">
      <stop offset="{ramp0:.4f}" stop-color="#fff" stop-opacity="0"/>
      <stop offset="{ramp1:.4f}" stop-color="#fff"/>
      <stop offset="{fade_out:.4f}" stop-color="#fff"/>
      <stop offset="1" stop-color="#fff" stop-opacity="0"/>
    </linearGradient>
    <mask id="wavemask" maskUnits="userSpaceOnUse" x="0" y="0" width="{w}" height="{H}"><rect width="{w}" height="{H}" fill="url(#wavefade)"/></mask>
    <mask id="beammask" maskUnits="userSpaceOnUse" x="0" y="0" width="{w}" height="{H}"><rect width="{w}" height="{H}" fill="url(#beamfade)"/></mask>

{wave_defs}
    <path id="beam" d="M0,{beam_y:.1f} H{w}"/>
    <g id="light">
{light(1, 1)}
    </g>
    <g id="lightin">
{light(t['inner'], t['beam_inner'])}
    </g>

    <filter id="glow" filterUnits="userSpaceOnUse" x="0" y="0" width="{w}" height="{H}" color-interpolation-filters="sRGB">
      <feGaussianBlur in="SourceGraphic" stdDeviation="{t['glow_wide']}" result="wideRaw"/>
      <feComponentTransfer in="wideRaw" result="wide"><feFuncA type="linear" slope="{t['glow_wide_a']}"/></feComponentTransfer>
      <feGaussianBlur in="SourceGraphic" stdDeviation="{t['glow_tight']}" result="tight"/>
      <feMerge>
        <feMergeNode in="wide"/><feMergeNode in="tight"/><feMergeNode in="SourceGraphic"/>
      </feMerge>
    </filter>

    {glass_filter(t, v, w, geo["maps"][v["look"]])}
  </defs>

  <!-- light travelling outside the glass -->
  <g mask="url(#outside)"><g filter="url(#glow)"><rect width="{w}" height="{H}" fill="#000" fill-opacity="0.004"/><use href="#light"/></g></g>

  <!-- the glass: body tint, then the light seen through it (the carrier fill only gives the filter its shape) -->
  <use href="#word" fill="url(#body)"/>
  <g filter="url(#glass)">
    <use href="#word" fill="#fff" fill-opacity="0.05"/>
    <g clip-path="url(#wordclip)"><use href="#lightin"/></g>
  </g>
</svg>
"""


STANDARD_INPUTS = {"SourceGraphic", "SourceAlpha", "BackgroundImage", "BackgroundAlpha", "FillPaint", "StrokePaint"}


def check(text, name):
    """Browsers silently ignore a filter input or url(#id) that resolves to nothing,
    which shows up only as a subtle visual glitch. Fail the build instead."""
    ns = "{http://www.w3.org/2000/svg}"
    root = ET.fromstring(text)
    problems = []
    for f in root.iter(ns + "filter"):
        seen = set()
        for prim in f:
            refs = [prim.get(a) for a in ("in", "in2")] + [n.get("in") for n in prim.findall(ns + "feMergeNode")]
            for ref in filter(None, refs):
                if ref not in STANDARD_INPUTS and ref not in seen:
                    problems.append(f'filter {f.get("id")}: {prim.tag.replace(ns, "")} reads "{ref}" before it is defined')
            if prim.get("result"):
                seen.add(prim.get("result"))
        if len(f) > MAX_PRIMITIVES:
            problems.append(f'filter {f.get("id")} has {len(f)} primitives; more than {MAX_PRIMITIVES} risks WebKit dropping it')
    for e in root.iter():
        href = e.get("href") or e.get("{http://www.w3.org/1999/xlink}href")
        if href and not href.startswith(("#", "data:")):
            problems.append(f"{e.tag.replace(ns, '')} loads {href[:40]!r}; GitHub renders README SVGs without external resources")
    ids = {e.get("id") for e in root.iter() if e.get("id")}
    for ref in set(re.findall(r'url\(#([^)]+)\)', text)) | set(re.findall(r'href="#([^"]+)"', text)):
        if ref not in ids:
            problems.append(f"reference to missing id {ref}")
    if problems:
        raise SystemExit(f"{name}:\n  " + "\n  ".join(problems))


def write(path, theme, variant, geometry):
    text = svg(theme, variant, geometry)
    check(text, path.name)
    path.write_text(text)
    print(f"wrote {path}")


def diag_svg():
    """Does feImage with a data: URI work here? The bar is displaced 20 units left of the red tick
    by a constant map. If it sits on the tick, the filter did nothing."""
    m = lens.png_constant((255, 128, 128))
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="240" height="120" viewBox="0 0 240 120">
  <defs><filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="240" height="120" color-interpolation-filters="sRGB">
    <feImage href="{m}" x="0" y="0" width="240" height="120" preserveAspectRatio="none" result="m"/>
    <feDisplacementMap in="SourceGraphic" in2="m" scale="40" xChannelSelector="R" yChannelSelector="G"/>
  </filter></defs>
  <rect width="240" height="120" fill="#fff" fill-opacity="0.85"/>
  <rect x="119" y="6" width="2" height="16" fill="#e0182d"/>
  <g filter="url(#f)"><rect x="115" y="30" width="10" height="80" fill="#111"/></g>
  <text x="120" y="118" font-family="sans-serif" font-size="9" text-anchor="middle" fill="#666">bar should sit LEFT of the red tick</text>
</svg>
"""


def options_readme(names):
    rows = []
    for n in names:
        rows.append(f"### {n}\n\n{VARIANTS[n]['desc']}\n\n"
                    f'<picture>\n  <source media="(prefers-color-scheme: dark)" srcset="logo-dark-{n}.svg">\n'
                    f'  <img alt="Discipline, {n} variant" src="logo-light-{n}.svg">\n</picture>\n')
    ab = ""
    if (HERE / "options" / "no-image" / "logo-dark.svg").exists():
        ab = ("\n### A/B: does `feImage` with a `data:` URI work on your device?\n\n"
              "The variants above bake refraction into PNGs and read them with `feImage`. `no-image/` is the previous "
              "build, which uses no `feImage` (kept only for this comparison; built by an earlier `build.py`, so "
              "not regenerated). If those render and the variants above look flat, `feImage` is the problem. "
              "The small test below shows it directly.\n\n"
              '<picture>\n  <source media="(prefers-color-scheme: dark)" srcset="no-image/logo-dark.svg">\n'
              '  <img alt="previous build, no feImage" src="no-image/logo-light.svg">\n</picture>\n\n'
              '<img alt="feImage diagnostic" src="diag-feimage.svg">\n')
    return ("# Logo options\n\nGenerated by `../build.py --options`. Each is served as a `<picture>` "
            "pair, so what you see follows your GitHub theme.\n\n" + "\n".join(rows) + ab)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("outdir", nargs="?", default=str(HERE))
    ap.add_argument("--font", default=str(DEFAULT_FONT))
    ap.add_argument("--options", action="store_true", help="write every variant into outdir/options")
    ap.add_argument("--variant", default=DEFAULT_VARIANT, choices=VARIANTS)
    a = ap.parse_args()
    out = pathlib.Path(a.outdir)
    geometry = layout(a.font)
    if a.options:
        opts = out / "options"
        opts.mkdir(exist_ok=True)
        for name in VARIANTS:
            for theme in THEMES:
                write(opts / f"logo-{theme}-{name}.svg", theme, name, geometry)
        (opts / "diag-feimage.svg").write_text(diag_svg())
        (opts / "README.md").write_text(options_readme(list(VARIANTS)))
    else:
        for theme in THEMES:
            write(out / f"logo-{theme}.svg", theme, a.variant, geometry)


if __name__ == "__main__":
    main()

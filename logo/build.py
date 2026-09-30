#!/usr/bin/env python3
"""Generate the Discipline logo as light/dark SVG pairs from one geometry.

Three phase-shifted sine waves (R, G, B) enter the left of the word, converge
as they pass through glass letter blocks, and leave the final "e" as one beam.

GitHub loads README SVGs through <img>: no web fonts, no external files, so the
word is outlined from the font and every effect is a self-contained SVG filter.

Safari: only long-standing filter primitives are used (feOffset, feComposite,
feFlood, feMerge, feGaussianBlur, feMorphology, feColorMatrix,
feComponentTransfer, feDisplacementMap). No mix-blend-mode, no feImage, no
lighting primitives. Every filter length is in user units, so nothing depends
on pixel density. NOT verified in WebKit; see logo/README.md.

usage: build.py [--font PATH] [outdir]            default logo-{dark,light}.svg
       build.py --options [outdir]                every variant + options/README.md
"""
import argparse
import math
import pathlib
import re
import xml.etree.ElementTree as ET

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

CHAMFER = 5       # px, width of the lit/shaded facets
# Refraction across the bezel: push strength at distance k*BAND_STEP from an
# edge, strongest at the edge (a convex bezel bends light most where it is steepest).
BAND_STEP = 2.5
BAND_PUSH = (1.0, 0.78, 0.5, 0.22)
BEND = 24         # px, shift at full push
CAST = (12, 14)   # px, where light spilled by the glass lands

# What each option changes. Keys not listed keep BASE_VARIANT's value.
BASE_VARIANT = dict(harmonics="single", prism=0.0, cast=False)
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
    "prism": dict(
        desc="Dispersion: red, green and blue bend by different amounts, so the beam fringes at every chamfer. "
             "Dark only; the light pair shows the plain bloom version.",
        prism=0.15,
    ),
    "caustic": dict(
        desc="Glass that spills light: a coloured caustic falls from the letters onto the page.",
        cast=True,
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


def outline_word(font_path):
    font = TTFont(font_path)
    cmap, glyphs, hmtx = font.getBestCmap(), font.getGlyphSet(), font["hmtx"]
    upm = font["head"].unitsPerEm
    advances = [hmtx[cmap[ord(ch)]][0] / upm for ch in WORD]
    em = WORD_WIDTH / (sum(advances) + TRACKING * (len(WORD) - 1))
    scale = em / upm
    x = MARGIN
    parts, spans = [], []
    for ch, adv in zip(WORD, advances):
        pen = SVGPathPen(glyphs, ntos=lambda v: f"{v:.1f}".rstrip("0").rstrip("."))
        glyphs[cmap[ord(ch)]].draw(TransformPen(pen, (scale, 0, 0, -scale, x, BASELINE)))
        parts.append(pen.getCommands())
        spans.append((x, x + adv * em))
        x += (adv + TRACKING) * em
    x_height = font["OS/2"].sxHeight * scale
    return " ".join(parts), spans, MARGIN + WORD_WIDTH + MARGIN, BASELINE - x_height / 2


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


def displacement_map():
    """Filter primitives for the refraction map. Grey 128 = no shift; each band
    pushes toward its own edge's outside, harder the closer it is to the edge.
    X and Y are built separately (opaque), then added into R and G."""
    out = []
    n = len(BAND_PUSH)

    def axis(name, sides, chan):
        # sides: (label, dx, dy, sign); the push is written into one channel.
        out.append(f'<feFlood flood-color="rgb(128,128,128)" result="{name}0"/>')
        layers = [f"{name}0"]
        for label, dx, dy, sign in sides:
            for k in range(n, 0, -1):  # widest, weakest band first
                d = k * BAND_STEP
                v = round(128 + sign * 127 * BAND_PUSH[k - 1])
                rgb = (v, 128, 128) if chan == "R" else (128, v, 128)
                out.append(f'<feOffset in="mask" dx="{dx * d}" dy="{dy * d}" result="{name}{label}{k}s"/>')
                out.append(f'<feComposite in="mask" in2="{name}{label}{k}s" operator="out" result="{name}{label}{k}b"/>')
                out.append(f'<feFlood flood-color="rgb{rgb}"/>')
                out.append(f'<feComposite in2="{name}{label}{k}b" operator="in" result="{name}{label}{k}"/>')
                layers.append(f"{name}{label}{k}")
        nodes = "".join(f'<feMergeNode in="{l}"/>' for l in layers)
        out.append(f'<feMerge result="{name}">{nodes}</feMerge>')

    axis("mx", (("L", 1, 0, +1), ("R", -1, 0, -1)), "R")
    axis("my", (("T", 0, 1, +1), ("B", 0, -1, -1)), "G")
    out.append('<feColorMatrix in="mx" type="matrix" values="1 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 0 1" result="mxR"/>')
    out.append('<feColorMatrix in="my" type="matrix" values="0 0 0 0 0  0 1 0 0 0  0 0 0 0 0  0 0 0 0 1" result="myG"/>')
    out.append('<feComposite in="mxR" in2="myG" operator="arithmetic" k1="0" k2="1" k3="1" k4="0" result="rawmap"/>')
    return out


def bend(spread):
    """Refract the light. With spread, each colour channel bends by a different amount."""
    disp = 'xChannelSelector="R" yChannelSelector="G"'
    if not spread:
        return [f'<feDisplacementMap in="light" in2="dmap" scale="{BEND}" {disp} result="bent"/>']
    out = []
    for ch, row, mult in (("R", "1 0 0 0 0  0 0 0 0 0  0 0 0 0 0", 1 - spread),
                          ("G", "0 0 0 0 0  0 1 0 0 0  0 0 0 0 0", 1.0),
                          ("B", "0 0 0 0 0  0 0 0 0 0  0 0 1 0 0", 1 + spread)):
        out.append(f'<feColorMatrix in="light" type="matrix" values="{row}  0 0 0 1 0" result="c{ch}"/>')
        out.append(f'<feDisplacementMap in="c{ch}" in2="dmap" scale="{BEND * mult:.1f}" {disp} result="d{ch}"/>')
    out.append('<feComposite in="dR" in2="dG" operator="arithmetic" k1="0" k2="1" k3="1" k4="0" result="dRG"/>')
    out.append('<feComposite in="dRG" in2="dB" operator="arithmetic" k1="0" k2="1" k3="1" k4="0" result="bent"/>')
    return out


def glass_filter(t, v, w):
    b = t["bloom"]
    p = []
    add = p.append
    add('<feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 30 0" result="mask"/>')
    # The letter-shaped carrier fill that gives this filter its shape is far fainter than any light.
    add('<feComponentTransfer in="SourceGraphic" result="light"><feFuncA type="linear" slope="1.4" intercept="-0.1"/></feComponentTransfer>')

    add("<!-- refraction map: 0.5 grey = flat, graded pushes toward each edge -->")
    p += displacement_map()
    add('<feGaussianBlur in="rawmap" stdDeviation="1.2" result="dmap"/>')
    p += bend(v["prism"] if t is THEMES["dark"] else 0)

    add("<!-- frost, then bloom: brighten the light and spread it, further along x than y -->")
    add(f'<feGaussianBlur in="bent" stdDeviation="{t["frost"]}" result="frost"/>')
    g, ag = b["gain"], b["alpha_gain"]
    add(f'<feColorMatrix in="bent" type="matrix" values="{g} 0 0 0 0  0 {g} 0 0 0  0 0 {g} 0 0  0 0 0 {ag} 0" result="hot"/>')
    for name in ("a", "b"):
        add(f'<feGaussianBlur in="hot" stdDeviation="{b[name][0]} {b[name][1]}" result="bloom{name.upper()}0"/>')
        add(f'<feComponentTransfer in="bloom{name.upper()}0" result="bloom{name.upper()}"><feFuncA type="linear" slope="{b[name + "_slope"]}"/></feComponentTransfer>')

    add("<!-- facets: top-left edges catch light, bottom-right edges take the tint -->")
    add(f'<feOffset in="mask" dx="{CHAMFER}" dy="{CHAMFER}" result="sTL"/>')
    add('<feComposite in="mask" in2="sTL" operator="out" result="litRaw"/>')
    add('<feGaussianBlur in="litRaw" stdDeviation="0.6" result="litShape"/>')
    add(f'<feFlood flood-color="{t["lit"]}" flood-opacity="{t["lit_opacity"]}"/>')
    add('<feComposite in2="litShape" operator="in" result="lit"/>')
    add(f'<feOffset in="mask" dx="-{CHAMFER}" dy="-{CHAMFER}" result="sBR"/>')
    add('<feComposite in="mask" in2="sBR" operator="out" result="shadeRaw"/>')
    add('<feGaussianBlur in="shadeRaw" stdDeviation="0.6" result="shadeShape"/>')
    add(f'<feFlood flood-color="{t["shade"]}" flood-opacity="{t["shade_opacity"]}"/>')
    add('<feComposite in2="shadeShape" operator="in" result="shade"/>')

    add("<!-- every side is chamfered; near a beam, the chamfer takes its colour -->")
    add(f'<feMorphology in="mask" operator="erode" radius="{CHAMFER}" result="core"/>')
    add('<feComposite in="mask" in2="core" operator="out" result="edgeShape"/>')
    add(f'<feFlood flood-color="{t["edge"]}" flood-opacity="{t["edge_opacity"]}"/>')
    add('<feComposite in2="edgeShape" operator="in" result="edge"/>')
    add('<feComposite in="bloomA" in2="edgeShape" operator="in" result="catchRaw"/>')
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


def svg(theme, variant, word_d, spans, width, beam_y):
    t, v = THEMES[theme], {**BASE_VARIANT, **VARIANTS[variant]}
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

    {glass_filter(t, v, w)}
  </defs>

  <!-- light travelling outside the glass -->
  <g mask="url(#outside)"><g filter="url(#glow)"><use href="#light"/></g></g>

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
    ids = {e.get("id") for e in root.iter() if e.get("id")}
    for ref in set(re.findall(r'url\(#([^)]+)\)', text)) | set(re.findall(r'href="#([^"]+)"', text)):
        if ref not in ids:
            problems.append(f"reference to missing id {ref}")
    if problems:
        raise SystemExit(f"{name}:\n  " + "\n  ".join(problems))


def write(path, theme, variant, geometry):
    text = svg(theme, variant, *geometry)
    check(text, path.name)
    path.write_text(text)
    print(f"wrote {path}")


def options_readme(names):
    rows = []
    for n in names:
        rows.append(f"### {n}\n\n{VARIANTS[n]['desc']}\n\n"
                    f'<picture>\n  <source media="(prefers-color-scheme: dark)" srcset="logo-dark-{n}.svg">\n'
                    f'  <img alt="Discipline, {n} variant" src="logo-light-{n}.svg">\n</picture>\n')
    return ("# Logo options\n\nGenerated by `../build.py --options`. Each is served as a `<picture>` "
            "pair, so what you see follows your GitHub theme.\n\n" + "\n".join(rows))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("outdir", nargs="?", default=str(HERE))
    ap.add_argument("--font", default=str(DEFAULT_FONT))
    ap.add_argument("--options", action="store_true", help="write every variant into outdir/options")
    ap.add_argument("--variant", default=DEFAULT_VARIANT, choices=VARIANTS)
    a = ap.parse_args()
    out = pathlib.Path(a.outdir)
    geometry = outline_word(a.font)
    if a.options:
        opts = out / "options"
        opts.mkdir(exist_ok=True)
        for name in VARIANTS:
            for theme in THEMES:
                write(opts / f"logo-{theme}-{name}.svg", theme, name, geometry)
        (opts / "README.md").write_text(options_readme(list(VARIANTS)))
    else:
        for theme in THEMES:
            write(out / f"logo-{theme}.svg", theme, a.variant, geometry)


if __name__ == "__main__":
    main()

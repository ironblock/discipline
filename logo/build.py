#!/usr/bin/env python3
"""Generate logo-dark.svg and logo-light.svg from one geometry.

Three phase-shifted sine waves (R, G, B) enter the left of the word, converge
as they pass through glass letter blocks, and leave the final "e" as one beam.

GitHub loads README SVGs through <img>: no web fonts, no external files, so the
word is outlined from the font and every effect is a self-contained SVG filter.

Safari: only long-standing filter primitives are used (feOffset, feComposite,
feFlood, feMerge, feGaussianBlur, feMorphology, feColorMatrix,
feDisplacementMap). No mix-blend-mode, no feImage, no lighting primitives.
The chamfer is built from offsets in user units, so it does not vary with
pixel density. NOT verified in WebKit; see logo/README.md.

usage: build.py [--font PATH] [outdir]
"""
import argparse
import pathlib

from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont

import math

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
STEP = 3          # px between polyline samples

CHAMFER = 5       # px, width of the lit/shaded facets
# Refraction across the bezel: push strength at distance k*BAND_STEP from an
# edge, strongest at the edge (a convex bezel bends light most where it is steepest).
BAND_STEP = 2.5
BAND_PUSH = (1.0, 0.78, 0.5, 0.22)
BEND = 24         # px, shift at full push

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
        frost=1.8, scatter=9, scatter_a=0.6,
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
        frost=1.8, scatter=7, scatter_a=0.4,
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


def wave_paths(spans, width, beam_y):
    start, end = convergence(spans)
    paths = []
    for k in range(3):
        phase = k * 2 * math.pi / 3
        pts, x = [], 0.0
        while x <= width + STEP:
            amp = AMPLITUDE * (1 - smoothstep(start, end, x))
            y = beam_y + amp * math.sin(2 * math.pi * x / WAVELENGTH + phase)
            pts.append(f"{x:.0f},{y:.1f}")
            x += STEP
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
    return "\n      ".join(out)


def svg(theme, word_d, spans, width, beam_y):
    t = THEMES[theme]
    start, end = convergence(spans)
    w = f"{width:.0f}"
    fade_in, fade_out = 110 / width, 1 - 150 / width
    ramp0, ramp1 = (end - 45) / width, (end + 25) / width
    strokes = "\n".join(
        f'      <path d="{d}" stroke="{c}"/>'
        for d, c in zip(wave_paths(spans, width, beam_y), t["wave"])
    )
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

    <g id="light">
      <g fill="none" stroke-width="5" stroke-linecap="round" stroke-linejoin="round" stroke-opacity="{t['wave_alpha']}" mask="url(#wavemask)">
{strokes}
      </g>
      <path d="M0,{beam_y:.1f} H{w}" stroke="{t['beam']}" stroke-width="{t['beam_width']}" stroke-linecap="round" mask="url(#beammask)"/>
    </g>

    <filter id="glow" filterUnits="userSpaceOnUse" x="0" y="0" width="{w}" height="{H}" color-interpolation-filters="sRGB">
      <feGaussianBlur in="SourceGraphic" stdDeviation="{t['glow_wide']}" result="wideRaw"/>
      <feComponentTransfer in="wideRaw" result="wide"><feFuncA type="linear" slope="{t['glow_wide_a']}"/></feComponentTransfer>
      <feGaussianBlur in="SourceGraphic" stdDeviation="{t['glow_tight']}" result="tight"/>
      <feMerge>
        <feMergeNode in="wide"/><feMergeNode in="tight"/><feMergeNode in="SourceGraphic"/>
      </feMerge>
    </filter>

    <filter id="glass" filterUnits="userSpaceOnUse" x="0" y="0" width="{w}" height="{H}" color-interpolation-filters="sRGB">
      <feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 30 0" result="mask"/>

      <!-- refraction map: 0.5 grey = flat, graded pushes toward each edge -->
      {displacement_map()}
      <feGaussianBlur in="rawmap" stdDeviation="1.2" result="dmap"/>
      <feDisplacementMap in="SourceGraphic" in2="dmap" scale="{BEND}" xChannelSelector="R" yChannelSelector="G" result="bent"/>

      <!-- frost: light scatters inside the block -->
      <feGaussianBlur in="bent" stdDeviation="{t['frost']}" result="frost"/>
      <feGaussianBlur in="bent" stdDeviation="{t['scatter']}" result="scatterRaw"/>
      <feComponentTransfer in="scatterRaw" result="scatter"><feFuncA type="linear" slope="{t['scatter_a']}"/></feComponentTransfer>

      <!-- facets: top-left edges catch light, bottom-right edges take the tint -->
      <feOffset in="mask" dx="{CHAMFER}" dy="{CHAMFER}" result="sTL"/>
      <feComposite in="mask" in2="sTL" operator="out" result="litRaw"/>
      <feGaussianBlur in="litRaw" stdDeviation="0.6" result="litShape"/>
      <feFlood flood-color="{t['lit']}" flood-opacity="{t['lit_opacity']}"/>
      <feComposite in2="litShape" operator="in" result="lit"/>
      <feOffset in="mask" dx="-{CHAMFER}" dy="-{CHAMFER}" result="sBR"/>
      <feComposite in="mask" in2="sBR" operator="out" result="shadeRaw"/>
      <feGaussianBlur in="shadeRaw" stdDeviation="0.6" result="shadeShape"/>
      <feFlood flood-color="{t['shade']}" flood-opacity="{t['shade_opacity']}"/>
      <feComposite in2="shadeShape" operator="in" result="shade"/>

      <!-- every side is chamfered, not only the two that face a light -->
      <feMorphology in="mask" operator="erode" radius="{CHAMFER}" result="core"/>
      <feComposite in="mask" in2="core" operator="out" result="edgeShape"/>
      <feFlood flood-color="{t['edge']}" flood-opacity="{t['edge_opacity']}"/>
      <feComposite in2="edgeShape" operator="in" result="edge"/>

      <!-- hairline rim -->
      <feMorphology in="mask" operator="erode" radius="0.9" result="inner"/>
      <feComposite in="mask" in2="inner" operator="out" result="rimShape"/>
      <feFlood flood-color="{t['rim']}" flood-opacity="{t['rim_opacity']}"/>
      <feComposite in2="rimShape" operator="in" result="rim"/>

      <feMerge result="all">
        <feMergeNode in="scatter"/><feMergeNode in="frost"/>
        <feMergeNode in="edge"/><feMergeNode in="shade"/><feMergeNode in="lit"/><feMergeNode in="rim"/>
      </feMerge>
      <feComposite in="all" in2="mask" operator="in"/>
    </filter>
  </defs>

  <!-- light travelling outside the glass -->
  <g mask="url(#outside)"><g filter="url(#glow)"><use href="#light"/></g></g>

  <!-- the glass: body tint, plus the same light seen through it -->
  <g filter="url(#glass)">
    <use href="#word" fill="url(#body)"/>
    <g clip-path="url(#wordclip)"><g filter="url(#glow)"><use href="#light"/></g></g>
  </g>
</svg>
"""


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("outdir", nargs="?", default=str(HERE))
    ap.add_argument("--font", default=str(DEFAULT_FONT))
    ap.add_argument("--suffix", default="")
    a = ap.parse_args()
    out = pathlib.Path(a.outdir)
    word_d, spans, width, beam_y = outline_word(a.font)
    for theme in THEMES:
        p = out / f"logo-{theme}{a.suffix}.svg"
        p.write_text(svg(theme, word_d, spans, width, beam_y))
        print(f"wrote {p}")


if __name__ == "__main__":
    main()

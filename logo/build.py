#!/usr/bin/env python3
"""Generate logo-dark.svg and logo-light.svg from one geometry.

Three phase-shifted sine waves (R, G, B) enter the left of the word, converge
as they pass through glass letter blocks, and leave the final "e" as one beam.
Dark: light adds (screen) toward white. Light: ink multiplies toward black.

GitHub loads README SVGs through <img>: no web fonts, no external files, so the
word is outlined from font/d-din-700.ttf (D-DIN Exp Bold, SIL OFL 1.1) and
every effect is a self-contained SVG filter.

usage: build.py [outdir]
"""
import math
import pathlib
import sys

from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont

HERE = pathlib.Path(__file__).parent
WORD = "Discipline"

FONT_SIZE = 170
TRACKING = 0.02  # em, added after each letter
MARGIN = 215
BASELINE = 218
H = 290
# Beam centreline: middle of the x-height band, where the lowercase letters sit.
BEAM_Y = BASELINE - 0.52 * FONT_SIZE / 2
AMPLITUDE = 56
WAVELENGTH = 120
STEP = 3  # px between polyline samples

THEMES = {
    "dark": dict(
        blend="screen",
        rgb=("#ff3040", "#22ff70", "#3f6bff"),
        body="#c6d0ff", body_top=0.13, body_bottom=0.03,
        lit="#ffffff", lit_opacity=0.80,
        shade="#8a7dff", shade_opacity=0.55,
        rim="#ffffff", rim_opacity=0.35,
        scatter=0.9, glow_wide=9, glow_wide_a=1.0, glow_tight=3,
        edge="#ffffff", edge_opacity=0.16,
    ),
    "light": dict(
        blend="multiply",
        rgb=("#e0182d", "#12b84a", "#2350e0"),
        body="#5b6690", body_top=0.05, body_bottom=0.16,
        lit="#ffffff", lit_opacity=0.95,
        shade="#1c2340", shade_opacity=0.60,
        rim="#1c2340", rim_opacity=0.55,
        scatter=0.5, glow_wide=2.5, glow_wide_a=0.22, glow_tight=0.7,
        edge="#1c2340", edge_opacity=0.14,
    ),
}

CHAMFER = 5  # px, width of the chamfer bands
BEND = 30    # px, peak refraction shift across a chamfer band


def outline_word():
    font = TTFont(HERE / "font" / "d-din-700.ttf")
    cmap, glyphs, hmtx = font.getBestCmap(), font.getGlyphSet(), font["hmtx"]
    scale = FONT_SIZE / font["head"].unitsPerEm
    x = MARGIN
    parts, spans = [], []
    for ch in WORD:
        name = cmap[ord(ch)]
        pen = SVGPathPen(glyphs, ntos=lambda v: f"{v:.1f}".rstrip("0").rstrip("."))
        glyphs[name].draw(TransformPen(pen, (scale, 0, 0, -scale, x, BASELINE)))
        parts.append(pen.getCommands())
        adv = hmtx[name][0] * scale
        spans.append((x, x + adv))
        x += adv + TRACKING * FONT_SIZE
    return " ".join(parts), spans, x - TRACKING * FONT_SIZE + MARGIN


def smoothstep(a, b, x):
    t = min(1.0, max(0.0, (x - a) / (b - a)))
    return t * t * (3 - 2 * t)


def waves(spans, width):
    """Return the three wave paths. Amplitude falls to zero across the word."""
    start = spans[0][0]
    end = spans[-1][0] + 0.35 * (spans[-1][1] - spans[-1][0])
    paths = []
    for k in range(3):
        phase = k * 2 * math.pi / 3
        pts = []
        x = 0.0
        while x <= width + STEP:
            amp = AMPLITUDE * (1 - smoothstep(start - 10, end, x))
            y = BEAM_Y + amp * math.sin(2 * math.pi * x / WAVELENGTH + phase)
            pts.append(f"{x:.0f},{y:.1f}")
            x += STEP
        paths.append("M" + " L".join(pts))
    return paths


def svg(theme, word_d, spans, width):
    t = THEMES[theme]
    wpaths = waves(spans, width)
    strokes = "\n".join(
        f'    <path d="{d}" stroke="{c}" style="mix-blend-mode:{t["blend"]}"/>'
        for d, c in zip(wpaths, t["rgb"])
    )
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{width:.0f}" height="{H}" viewBox="0 0 {width:.0f} {H}" role="img" aria-label="Discipline">
  <title>Discipline</title>
  <defs>
    <path id="word" d="{word_d}"/>
    <clipPath id="wordclip"><use href="#word"/></clipPath>
    <mask id="outside" maskUnits="userSpaceOnUse" x="0" y="0" width="{width:.0f}" height="{H}">
      <rect width="{width:.0f}" height="{H}" fill="#fff"/>
      <use href="#word" fill="#000"/>
    </mask>

    <linearGradient id="body" gradientUnits="userSpaceOnUse" x1="0" y1="{BASELINE - 130}" x2="0" y2="{BASELINE + 35}">
      <stop offset="0" stop-color="{t['body']}" stop-opacity="{t['body_top']}"/>
      <stop offset="1" stop-color="{t['body']}" stop-opacity="{t['body_bottom']}"/>
    </linearGradient>
    <linearGradient id="fadeg" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="{width:.0f}" y2="0">
      <stop offset="0" stop-color="#fff" stop-opacity="0"/>
      <stop offset="{110/width:.4f}" stop-color="#fff"/>
      <stop offset="{1 - 150/width:.4f}" stop-color="#fff"/>
      <stop offset="1" stop-color="#fff" stop-opacity="0"/>
    </linearGradient>
    <mask id="fade" maskUnits="userSpaceOnUse" x="0" y="0" width="{width:.0f}" height="{H}">
      <rect width="{width:.0f}" height="{H}" fill="url(#fadeg)"/>
    </mask>

    <g id="beams" fill="none" stroke-width="5" stroke-linecap="round" stroke-linejoin="round" style="isolation:isolate">
{strokes}
    </g>

    <filter id="glow" filterUnits="userSpaceOnUse" x="0" y="0" width="{width:.0f}" height="{H}" color-interpolation-filters="sRGB">
      <feGaussianBlur in="SourceGraphic" stdDeviation="{t['glow_wide']}" result="wideRaw"/>
      <feComponentTransfer in="wideRaw" result="wide"><feFuncA type="linear" slope="{t['glow_wide_a']}"/></feComponentTransfer>
      <feGaussianBlur in="SourceGraphic" stdDeviation="{t['glow_tight']}" result="tight"/>
      <feMerge>
        <feMergeNode in="wide"/><feMergeNode in="tight"/><feMergeNode in="SourceGraphic"/>
      </feMerge>
    </filter>

    <filter id="glass" filterUnits="userSpaceOnUse" x="0" y="0" width="{width:.0f}" height="{H}" color-interpolation-filters="sRGB">
      <!-- The chamfer is built from offsets in user units, so it looks the same at any pixel density. -->
      <feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 30 0" result="mask"/>

      <!-- refraction map: 0.5 grey = flat; each chamfer band pushes toward its own side -->
      <feOffset in="mask" dx="{CHAMFER}" dy="0" result="sR"/><feComposite in="mask" in2="sR" operator="out" result="bandL"/>
      <feOffset in="mask" dx="-{CHAMFER}" dy="0" result="sL"/><feComposite in="mask" in2="sL" operator="out" result="bandR"/>
      <feOffset in="mask" dx="0" dy="{CHAMFER}" result="sD"/><feComposite in="mask" in2="sD" operator="out" result="bandT"/>
      <feOffset in="mask" dx="0" dy="-{CHAMFER}" result="sU"/><feComposite in="mask" in2="sU" operator="out" result="bandB"/>
      <feFlood flood-color="rgb(128,128,128)" result="flat"/>
      <feFlood flood-color="rgb(255,128,128)"/><feComposite in2="bandL" operator="in" result="pushL"/>
      <feFlood flood-color="rgb(0,128,128)"/><feComposite in2="bandR" operator="in" result="pushR"/>
      <feFlood flood-color="rgb(128,255,128)"/><feComposite in2="bandT" operator="in" result="pushT"/>
      <feFlood flood-color="rgb(128,0,128)"/><feComposite in2="bandB" operator="in" result="pushB"/>
      <feMerge result="rawmap">
        <feMergeNode in="flat"/><feMergeNode in="pushL"/><feMergeNode in="pushR"/><feMergeNode in="pushT"/><feMergeNode in="pushB"/>
      </feMerge>
      <feGaussianBlur in="rawmap" stdDeviation="1.6" result="dmap"/>
      <feDisplacementMap in="SourceGraphic" in2="dmap" scale="{BEND}" xChannelSelector="R" yChannelSelector="G" result="bent"/>

      <!-- frost: light scatters inside the block -->
      <feGaussianBlur in="bent" stdDeviation="1.1" result="frost"/>
      <feGaussianBlur in="bent" stdDeviation="8" result="scatterRaw"/>
      <feComponentTransfer in="scatterRaw" result="scatter"><feFuncA type="linear" slope="{t['scatter']}"/></feComponentTransfer>

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
  <g mask="url(#outside)"><g filter="url(#glow)"><g mask="url(#fade)"><use href="#beams"/></g></g></g>

  <!-- the glass: body tint, plus the same light seen through it -->
  <g filter="url(#glass)">
    <use href="#word" fill="url(#body)"/>
    <g clip-path="url(#wordclip)"><g filter="url(#glow)"><g mask="url(#fade)"><use href="#beams"/></g></g></g>
  </g>
</svg>
"""


def main():
    out = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else HERE
    word_d, spans, width = outline_word()
    for theme in THEMES:
        (out / f"logo-{theme}.svg").write_text(svg(theme, word_d, spans, width))
        print(f"wrote {out / f'logo-{theme}.svg'}")


if __name__ == "__main__":
    main()

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

import numpy as np

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
WAVELENGTH = 190  # px; the waves' peak-to-peak height is the font's x-height (see layout)

# A 104-primitive glass filter (4-band stair refraction map) painted nothing in iOS Safari and in
# WebKitGTK 2.52. The same graph with 3 bands (88) painted, and padding a working graph to 104 with
# identity primitives also painted, so this is not a plain count and the mechanism is unknown.
# This ceiling is a heuristic: the working graphs are under 60. Re-check in WebKit if it grows.
MAX_PRIMITIVES = 64
PAD = 16          # px around the letters covered by the baked maps
CROP = 26         # px of canvas kept above and below the word
CROSS_OUTSIDE = 6  # px outside the D's left edge where red and green cross; 0 would put the crossing on the edge and the glass hides half of it
ENDS_FADE = 22    # px over which the outside light fades out beside the first and last letter (about a stem)

# What each option changes. Keys not listed keep BASE_VARIANT's value.
BASE_VARIANT = dict(harmonics="single", look="carved", taper="mid", glass="elements", margin=MARGIN, outside=True,
                    amp=1.0, cross=CROSS_OUTSIDE, theme={}, palette="legacy")
VARIANTS = {
    "elements": dict(
        desc="The default. Glass built from elements (no displacement filter), a taper halfway between early "
             "and late, and a stronger, blurrier, more specular look than before.",
    ),
    "early": dict(
        desc="The waves calm almost at once and leave a long quiet tail (quadratic ease-out).",
        taper="early",
    ),
    "late": dict(
        desc="The waves stay lively through most of the word and settle into the `e` (quadratic ease-in).",
        taper="late",
    ),
    "noise": dict(
        desc="Signal from noise: each waveform carries harmonics that the glass strips away letter by letter, "
             "so the tangle on the left resolves into one line.",
        harmonics="filter",
    ),
    "story": dict(
        desc="The light enters the `D` and leaves the `e`, and between them it is visible only inside the letters: "
             "the gaps and counters are dark. The incoming waves and the outgoing beam are drawn as usual.",
        outside="ends",
    ),
    "story-noise": dict(
        desc="Both ideas: a tangle of harmonics enters the `D`, the glass strips them letter by letter, and between "
             "the first and last letter the light is visible only inside the glass. One clean beam leaves the `e`.",
        harmonics="filter", outside="ends",
    ),
    "inside": dict(
        desc="The letters are a window and nothing is drawn outside them, not even the ends. The canvas is cropped "
             "to the word.",
        margin=28, outside=False,
    ),
    "refract": dict(
        desc="The `elements` glass, but the waves are bent by a displacement filter (one `feImage` map, one "
             "`feDisplacementMap`) instead of at build time, so the lines stay plain paths a future animation can move. "
             "True refraction strength.",
        glass="refract", gain=1.0,
    ),
    "refract-strong": dict(
        desc="`refract` with the displacement scaled up (`REFRACT_GAIN`), as `elements` exaggerates it.",
        glass="refract",
    ),
    "refract-liquid": dict(
        desc="`refract-strong` with the wide, thick `liquid` bezel, which throws light much further at the rim.",
        glass="refract", look="liquid",
    ),
    "refract-story-noise": dict(
        desc="`story-noise` with the waves bent by the displacement filter at `REFRACT_GAIN`.",
        glass="refract", harmonics="filter", outside="ends",
    ),
    "tuned": dict(
        desc="`refract-story-noise` as tuned in the playground: displacement-filter glass with a thicker, denser "
             "bezel, lower waves and a late taper, drawn in the layered palette (PALETTE and DELTA).",
        glass="refract", look="tuned", harmonics="filter", outside="ends", taper="late", gain=3.45, amp=0.68, cross=4,
        palette="layered",
    ),
    "filter-glass": dict(
        desc="The previous glass, which bends the light with a displacement filter and two baked maps. Renders on "
             "iPhone Safari via GitHub. Kept as a fallback and a comparison.",
        glass="filter",
    ),
}
DEFAULT_VARIANT = "tuned"

THEMES = {
    "dark": dict(
        wave=("#ffa3b3", "#a3ffc6", "#a9c0ff"), wave_alpha=0.80,
        beam="#ffffff", beam_width=6, pad="#000",
        glow_wide=9, glow_wide_a=1.0, glow_tight=3,
        body="#f0f3ff", body_top=0.44, body_bottom=0.24,
        lit="#ffffff", lit_opacity=0.75,
        shade="#8a7dff", shade_opacity=0.50,
        edge="#ffffff", edge_opacity=0.16,
        rim="#ffffff", rim_opacity=0.35,
        frost=6, soft=2.8,
        inner=1.5,  # strokes are this much thicker inside the glass, as if focused
        beam_inner=1.5,
        # gain/alpha_gain: brighten the light before it is spread. a, b: near and far
        # blur (x, y); wider in x, along the travel direction. slopes: how much of each
        # survives. catch: how strongly the chamfers pick up colour.
        bloom=dict(gain=1.15, alpha_gain=1.9, a=(9, 5.5), a_slope=0.9, b=(32, 12), b_slope=2.1,
                   catch=2.8),
        # element glass: width and opacity of the layered copies that stand in for the bloom blurs
        el=dict(a_width=2.4, a_alpha=0.7, b_width=4.5, b_alpha=0.75, bevel=1.0),
    ),
    "light": dict(
        wave=("#e0182d", "#12b84a", "#2350e0"), wave_alpha=0.50,
        beam="#0b1020", beam_width=6, pad="#fff",
        glow_wide=2.5, glow_wide_a=0.22, glow_tight=0.7,
        body="#2c3560", body_top=0.36, body_bottom=0.56,
        lit="#ffffff", lit_opacity=0.95,
        shade="#1c2340", shade_opacity=0.60,
        edge="#1c2340", edge_opacity=0.14,
        rim="#1c2340", rim_opacity=0.55,
        frost=6, soft=2.8,
        inner=1.5,
        beam_inner=0.9,  # a black beam's bloom reads as smoke, not glow
        bloom=dict(gain=1.0, alpha_gain=1.0, a=(9, 5.5), a_slope=0.6, b=(32, 12), b_slope=0.6,
                   catch=1.0),
        el=dict(a_width=2.6, a_alpha=0.55, b_width=5.0, b_alpha=0.45, bevel=0.8),
    ),
}


def layered(*layers):
    """Merge dicts left to right, nested dicts key by key; a tuple, list or scalar replaces what is below it."""
    out = {}
    for layer in layers:
        for k, v in layer.items():
            out[k] = layered(out[k], v) if isinstance(v, dict) and isinstance(out.get(k), dict) else v
    return out


# The shipped design's palettes (variant palette="layered"), tuned in the playground: what both themes
# share, then what each theme changes. Everything the refract glass draws with, and nothing else.
PALETTE = dict(
    wave_alpha=0.85, beam_width=6, lit="#ffffff",
    frost=3.1, soft=0.9, inner=1.05,
    bloom=dict(a=(9, 5.5), b=(32, 12.5)),
)
DELTA = {
    "dark": dict(
        wave=("#ffa3b3", "#a3ffc6", "#a9c0ff"), beam="#ffffff", pad="#000",
        glow_wide=9, glow_wide_a=0.16, glow_tight=6.3,
        body="#f0f3ff", body_top=0.01, body_bottom=0.29,
        lit_opacity=0.78, shade="#8a7dff", shade_opacity=0.63,
        edge="#ffffff", edge_opacity=0.15, rim="#ffffff", rim_opacity=0.5,
        beam_inner=2.05,
        el=dict(a_width=4, a_alpha=0.35, b_width=1.9, b_alpha=0.7, bevel=1.7),
    ),
    "light": dict(
        wave=("#f07581", "#74f19e", "#7994ec"), beam="#4a4f5e", pad="#fff",
        glow_wide=2.5, glow_wide_a=0.05, glow_tight=1.5,
        body="#cbd0ec", body_top=0.03, body_bottom=0.42,
        lit_opacity=0.97, shade="#414e81", shade_opacity=0.75,
        edge="#737b9c", edge_opacity=0.13, rim="#9198b6", rim_opacity=0.75,
        beam_inner=1.25,
        el=dict(a_width=4.3, a_alpha=0.4, b_width=2.1, b_alpha=0.55, bevel=1.35),
    ),
}


def palette(theme, v):
    """The palette a variant draws with in a theme."""
    if v["palette"] == "layered":
        return layered(PALETTE, DELTA[theme])
    return layered(THEMES[theme], v["theme"].get(theme, {}))


def layout(font_path, margin=MARGIN):
    """Positions the word once. The SVG path and the glyph contours the maps are baked
    from both come from here."""
    font = TTFont(font_path)
    cmap, glyphs, hmtx = font.getBestCmap(), font.getGlyphSet(), font["hmtx"]
    upm = font["head"].unitsPerEm
    advances = [hmtx[cmap[ord(ch)]][0] / upm for ch in WORD]
    em = WORD_WIDTH / (sum(advances) + TRACKING * (len(WORD) - 1))
    scale = em / upm
    x = margin
    parts, spans, contours, coarse = [], [], [], []
    for ch, adv in zip(WORD, advances):
        name = cmap[ord(ch)]
        pen = SVGPathPen(glyphs, ntos=lambda v: f"{v:.1f}".rstrip("0").rstrip("."))
        glyphs[name].draw(TransformPen(pen, (scale, 0, 0, -scale, x, BASELINE)))
        parts.append(pen.getCommands())
        contours += lens.glyph_contours(glyphs, name, x, BASELINE, scale)
        coarse += lens.glyph_contours(glyphs, name, x, BASELINE, scale, steps=6)
        spans.append((x, x + adv * em))
        x += (adv + TRACKING) * em
    xs = [p[0] for c in contours for p in c]
    ys = [p[1] for c in contours for p in c]
    region = (math.floor(min(xs)) - PAD, math.floor(min(ys)) - PAD,
              math.ceil(max(xs)) - math.floor(min(xs)) + 2 * PAD, math.ceil(max(ys)) - math.floor(min(ys)) + 2 * PAD)
    x_height = font["OS/2"].sxHeight * scale
    beam_y = BASELINE - x_height / 2
    maps = {name: lens.maps(contours, region, look) for name, look in lens.LOOKS.items()}
    # where the beam's row enters the first letter and leaves the last: the light is cut off there
    xs_row = np.arange(region[0], region[0] + region[2], 0.25)
    hit = np.flatnonzero(lens.inside(maps["carved"]["field"], np.column_stack([xs_row, np.full_like(xs_row, beam_y)])))
    stem_end = hit[0] + int(np.flatnonzero(np.diff(hit) > 1)[0])       # last sample of the first run: the D's stem
    return dict(d=" ".join(parts), spans=spans, width=margin + WORD_WIDTH + margin, beam_y=beam_y,
                x_height=x_height, contours=coarse, maps=maps, enters=xs_row[hit[0]], leaves=xs_row[hit[-1]] + 0.25,
                spine=(xs_row[hit[0]] + xs_row[stem_end] + 0.25) / 2)


# amplitude = (1 - t**a) ** b, with t the progress along the taper. "late" is 1 - t^2 and "early" is
# (1 - t)^2; their arithmetic mean is exactly linear, so "mid" splits the difference in the exponents.
TAPERS = {"late": (2, 1), "mid": (1.5, 1.5), "early": (1, 2)}


def taper(kind, t):
    """How much of the amplitude is gone at progress t (0 to 1) along the taper."""
    a, b = TAPERS[kind]
    return 1 - (1 - min(1.0, max(0.0, t)) ** a) ** b


def convergence(spans):
    """x where amplitude starts to fall, and x where it has reached zero."""
    return spans[0][0] - 10, spans[-1][0] + 0.55 * (spans[-1][1] - spans[-1][0])


def harmonics(spans, mode, amplitude):
    """(period, amplitude, fade start x, fade end x) for each component of a wave.
    In "filter" mode the shorter components are stripped early, one letter at a time."""
    start, end = convergence(spans)
    if mode == "single":
        return [(WAVELENGTH, amplitude, start, end)]
    return [
        (WAVELENGTH, 0.62 * amplitude, start, end),
        (0.53 * WAVELENGTH, 0.26 * amplitude, spans[0][0], spans[3][1]),
        (0.33 * WAVELENGTH, 0.12 * amplitude, spans[0][0], spans[1][1]),
    ]


def wave_y(comps, kind, k, x, beam_y, shift=0.0):
    """The k-th wave at x. shift slides the oscillation along x without moving the taper."""
    y = beam_y
    for h, (period, amp, fade0, fade1) in enumerate(comps):
        phase = k * 2 * math.pi / 3 * (1 + h) + 0.9 * h * k
        y += amp * (1 - taper(kind, (x - fade0) / (fade1 - fade0))) * math.sin(2 * math.pi * (x + shift) / period + phase)
    return y


def crossover_shift(comps, kind, beam_y, x):
    """The smallest slide of the oscillation that puts the red and green waves on top of each other at x.
    Solved numerically, because the noise harmonics move the crossing away from the pure-sine answer."""
    period = comps[0][0]
    gap = lambda sh: wave_y(comps, kind, 0, x, beam_y, sh) - wave_y(comps, kind, 1, x, beam_y, sh)
    grid = np.linspace(-period / 2, period / 2, 2001)
    vals = np.array([gap(g) for g in grid])
    roots = []
    for i in np.flatnonzero(vals[:-1] * vals[1:] < 0):                # sign changes: true crossings
        lo, hi = grid[i], grid[i + 1]
        for _ in range(60):
            mid = (lo + hi) / 2
            lo, hi = (mid, hi) if gap(lo) * gap(mid) > 0 else (lo, mid)
        roots.append((lo + hi) / 2)
    return min(roots, key=abs)


def wave_paths(comps, kind, width, beam_y, step, shift=0.0):
    paths = []
    for k in range(3):
        pts = [f"{x:.0f},{wave_y(comps, kind, k, x, beam_y, shift):.1f}" for x in np.arange(0, width + step, step)]
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

    add('<feMerge><feMergeNode in="insideClip"/></feMerge>')

    body = "\n      ".join(p)
    return (f'<filter id="glass" filterUnits="userSpaceOnUse" x="0" y="0" width="{w}" height="{H}" '
            f'color-interpolation-filters="sRGB">\n      {body}\n    </filter>')


def runs_path(pts, keep):
    """SVG path data for the consecutive runs of pts where keep is true."""
    out, run = [], []
    for p, k in zip(pts, keep):
        if k:
            run.append(f"{p[0]:.1f},{p[1]:.1f}")
        elif run:
            out.append("M" + " L".join(run))
            run = []
    if run:
        out.append("M" + " L".join(run))
    return " ".join(out)


REFRACT_GAIN = 1.6  # the strokes are thin, so the true shift barely reads; exaggerate it


def refracted_paths(geo, comps, kind, field, shift):
    """Bend each wave, and the beam, the way the displacement filter would: content inside a bezel is
    seen shifted toward the edge, so the path is moved by the field there. Points outside the
    letters stay put and are not drawn here (the outside light is drawn as usual)."""
    lo, hi = geo["spans"][0][0] - 20, geo["spans"][-1][1] + 20
    xs = np.arange(lo, hi, 1.5)
    paths = {}
    for k in range(3):
        pts = np.array([[x, wave_y(comps, kind, k, x, geo["beam_y"], shift)] for x in xs])
        paths[f"r{k}"] = runs_path(lens.refract(field, pts, REFRACT_GAIN), lens.inside(field, pts))
    pts = np.array([[x, geo["beam_y"]] for x in xs])
    paths["rb"] = runs_path(lens.refract(field, pts, REFRACT_GAIN), lens.inside(field, pts))
    return paths


def falloff_layers(reach, steps=5):
    """(stroke width, opacity share) pairs that stack into the filter glass's rim falloff, 1 - smoothstep(0, reach, d).
    Strokes are clipped to the letter, so a stroke of width 2r shows r of it."""
    def smoothstep(x):
        return x * x * (3 - 2 * x)
    cover = [1 - smoothstep((k - 0.5) / steps) for k in range(1, steps + 1)] + [0]    # opacity where layer k is the outermost
    return [(2 * reach * k / steps, cover[k - 1] - cover[k]) for k in range(1, steps + 1)]


def bevel_paths(geo, field, t, look):
    """Rim light as stroked outline segments. Each segment takes its brightness from how squarely its
    outward normal faces the light (top-left) or the opposite side (bottom-right), in QUANT steps.
    Returns path data by id; the strokes are clipped to the letters, so half of each shows."""
    QUANT = 12
    buckets = {}
    light = np.array(lens.LIGHT)
    for c in geo["contours"]:
        pts = np.array(c + [c[0]])
        mids = (pts[:-1] + pts[1:]) / 2
        inward = np.stack([lens.sample(field, "ux", mids), lens.sample(field, "uy", mids)], 1)
        toward = -(inward @ light)
        for kind, amount in (("l", np.clip(toward, 0, 1)), ("s", np.clip(-toward, 0, 1))):
            level = np.rint(amount ** look["spec_power"] * QUANT).astype(int)
            for i, q in enumerate(level):
                if q:
                    key = f"b{kind}{q}"
                    seg = buckets.setdefault(key, [])
                    if seg and seg[-1][1] == i:      # continues the previous segment of this bucket
                        seg[-1] = (seg[-1][0] + [tuple(pts[i + 1])], i + 1)
                    else:
                        seg.append(([tuple(pts[i]), tuple(pts[i + 1])], i + 1))
    return {k: " ".join("M" + " L".join(f"{x:.1f},{y:.1f}" for x, y in pl) for pl, _ in segs) for k, segs in buckets.items()}, QUANT


def bend_filter(maps, gain, w):
    """Displace whatever it is applied to by the baked refraction map: the whole of the glass's
    refraction, in two primitives."""
    rx, ry, rw, rh = maps["region"]
    return (f'<filter id="bend" filterUnits="userSpaceOnUse" x="0" y="0" width="{w}" height="{H}" color-interpolation-filters="sRGB">'
            f'<feImage href="{maps["disp"]}" x="{rx}" y="{ry}" width="{rw}" height="{rh}" preserveAspectRatio="none" result="dmap"/>'
            f'<feDisplacementMap in="SourceGraphic" in2="dmap" scale="{maps["scale"] * gain:.2f}" xChannelSelector="R" yChannelSelector="G"/>'
            '</filter>')


def element_glass(t, v, geo, comps, w):
    """Glass whose rim light is outline strokes. The waves are bent either at build time (glass="elements")
    or by the `bend` displacement filter (glass="refract"). Returns (defs, body)."""
    look = lens.LOOKS[v["look"]]
    maps = geo["maps"][v["look"]]
    field = maps["field"]
    bend = v["glass"] == "refract"
    paths = {} if bend else refracted_paths(geo, comps, v["taper"], field, geo["shift"])
    bev, quant = bevel_paths(geo, field, t, look)
    b, el = t["bloom"], t["el"]
    defs = [f'<path id="{k}" d="{d}"/>' for k, d in {**paths, **bev}.items() if d]
    if bend:
        defs.append(bend_filter(maps, v.get("gain", REFRACT_GAIN), w))
    for name, dev in (("soft", f'{t["soft"]}'), ("frost", f'{t["frost"]}'), ("bloomA", f'{b["a"][0]} {b["a"][1]}'), ("bloomB", f'{b["b"][0]} {b["b"][1]}')):
        defs.append(f'<filter id="{name}" filterUnits="userSpaceOnUse" x="0" y="0" width="{w}" height="{H}" '
                    f'color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="{dev}"/></filter>')

    def light(scale, beam_scale, alpha):
        prefix, beam_id = ("w", "beam") if bend else ("r", "rb")
        strokes = "\n".join(f'          <use href="#{prefix}{k}" stroke="{c}"/>' for k, c in enumerate(t["wave"]))
        group = (f'<g opacity="{alpha:g}">\n'
                 f'        <g fill="none" stroke-width="{5 * scale:g}" stroke-linecap="round" stroke-linejoin="round" '
                 f'stroke-opacity="{t["wave_alpha"]}" mask="url(#wavemask)">\n{strokes}\n        </g>\n'
                 f'        <use href="#{beam_id}" fill="none" stroke="{t["beam"]}" stroke-width="{t["beam_width"] * beam_scale:g}" '
                 f'stroke-linecap="round" mask="url(#beammask)"/>\n      </g>')
        if not bend:
            return group
        # the padding rect keeps WebKit from clipping the filter to the lines' own box
        return (f'<g filter="url(#bend)"><rect width="{w}" height="{H}" fill="{t["pad"]}" fill-opacity="0.004"/>'
                f'{group}</g>')

    inner, binner = t["inner"], t["beam_inner"]
    facets = []
    layers = falloff_layers(look["spec_width"])
    if el.get("core"):                                   # optional crisp highlight on the lit side only
        lit_layers = layers + [(el["core"], 1.0)]
    else:
        lit_layers = layers
    for kind, colour, opacity, kind_layers in (("s", t["shade"], t["shade_opacity"], layers),
                                               ("l", t["lit"], t["lit_opacity"], lit_layers)):
        for q in range(1, quant + 1):
            if f"b{kind}{q}" in bev:
                level = q / quant
                for width, share in kind_layers:
                    facets.append(f'<use href="#b{kind}{q}" fill="none" stroke="{colour}" '
                                  f'stroke-opacity="{opacity * level * share * el["bevel"]:.3f}" '
                                  f'stroke-width="{width:.2f}" stroke-linejoin="round"/>')
    body = f"""<use href="#word" fill="url(#body)"/>
  <g clip-path="url(#wordclip)">
    <use href="#word" fill="none" stroke="{t['edge']}" stroke-width="{2 * look['band_width']:g}" stroke-opacity="{t['edge_opacity']}"/>
    <g filter="url(#bloomB)">{light(inner * el['b_width'], binner * el['b_width'], el['b_alpha'])}</g>
    <g filter="url(#bloomA)">{light(inner * el['a_width'], binner * el['a_width'], el['a_alpha'])}</g>
    <g filter="url(#frost)">{light(inner, binner, 1)}</g>
    <g filter="url(#soft)">
      {chr(10).join("      " + f for f in facets).strip()}
    </g>
    <use href="#word" fill="none" stroke="{t['rim']}" stroke-width="1.8" stroke-opacity="{t['rim_opacity']}"/>
  </g>"""
    return "\n    ".join(defs), body


def svg(theme, variant, geo):
    v = {**BASE_VARIANT, **VARIANTS[variant]}
    t = palette(theme, v)
    word_d, spans, width, beam_y = geo["d"], geo["spans"], geo["width"], geo["beam_y"]
    start, end = convergence(spans)
    w = f"{width:.0f}"
    fade_in, fade_out = (110 if v['outside'] else 1) / width, 1 - (150 if v['outside'] else 1) / width
    ramp0, ramp1 = (end - 45) / width, (end + 25) / width
    comps = harmonics(spans, v["harmonics"], geo["x_height"] / 2 * v["amp"])
    geo = {**geo, "shift": crossover_shift(comps, v["taper"], beam_y, geo["enters"] - v["cross"])}
    wave_defs = "\n".join(
        f'    <path id="w{k}" d="{d}"/>'
        for k, d in enumerate(wave_paths(comps, v["taper"], width, beam_y, 3 if v["harmonics"] == "single" else 2, geo["shift"]))
    )
    if v["glass"] in ("elements", "refract"):
        glass_defs, glass_body = element_glass(t, v, geo, comps, w)
    else:
        glass_defs = glass_filter(t, v, w, geo["maps"][v["look"]])
        glass_body = f"""<use href="#word" fill="url(#body)"/>
  <g filter="url(#glass)">
    <use href="#word" fill="#fff" fill-opacity="0.05"/>
    <g clip-path="url(#wordclip)"><use href="#lightin"/></g>
  </g>"""

    def light(scale, beam_scale):
        strokes = "\n".join(f'        <use href="#w{k}" stroke="{c}"/>' for k, c in enumerate(t["wave"]))
        return f"""      <g fill="none" stroke-width="{5 * scale:g}" stroke-linecap="round" stroke-linejoin="round" stroke-opacity="{t['wave_alpha']}" mask="url(#wavemask)">
{strokes}
      </g>
      <use href="#beam" fill="none" stroke="{t['beam']}" stroke-width="{t['beam_width'] * beam_scale:g}" stroke-linecap="round" mask="url(#beammask)"/>"""

    ends_mask = ""
    if v["outside"] == "ends":
        e, l = geo["enters"], geo["leaves"]
        ends_mask = (
            f'    <linearGradient id="endsL" gradientUnits="userSpaceOnUse" x1="{e:.2f}" y1="0" x2="{e + ENDS_FADE:.2f}" y2="0">'
            '<stop offset="0" stop-color="#fff"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></linearGradient>\n'
            f'    <linearGradient id="endsR" gradientUnits="userSpaceOnUse" x1="{l - ENDS_FADE:.2f}" y1="0" x2="{l:.2f}" y2="0">'
            '<stop offset="0" stop-color="#fff" stop-opacity="0"/><stop offset="1" stop-color="#fff"/></linearGradient>\n'
            f'    <mask id="ends" maskUnits="userSpaceOnUse" x="0" y="0" width="{w}" height="{H}">\n'
            f'      <rect x="0" y="0" width="{e:.2f}" height="{H}" fill="#fff"/>\n'
            f'      <rect x="{e:.2f}" y="0" width="{ENDS_FADE}" height="{H}" fill="url(#endsL)"/>\n'
            f'      <rect x="{l - ENDS_FADE:.2f}" y="0" width="{ENDS_FADE}" height="{H}" fill="url(#endsR)"/>\n'
            f'      <rect x="{l:.2f}" y="0" width="{width - l:.2f}" height="{H}" fill="#fff"/>\n'
            f'      <use href="#word" fill="#000"/>\n'
            f'    </mask>\n')
    outside_light = ("  <!-- light travelling outside the glass -->\n"
                     f'  <g mask="url(#{"ends" if v["outside"] == "ends" else "outside"})"><g filter="url(#glow)"><rect width="{w}" height="{H}" fill="{t["pad"]}" fill-opacity="0.004"/><use href="#light"/></g></g>\n'
                     if v["outside"] else "")
    rx, ry, rw, rh = geo["maps"][v["look"]]["region"]
    top, height = ry + PAD - CROP, rh - 2 * PAD + 2 * CROP      # the word's own extent plus CROP either side
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{height}" viewBox="0 {top} {w} {height}" role="img" aria-label="Discipline">
  <title>Discipline</title>
  <defs>
    <path id="word" d="{word_d}"/>
    <clipPath id="wordclip"><use href="#word"/></clipPath>
    <mask id="outside" maskUnits="userSpaceOnUse" x="0" y="0" width="{w}" height="{H}">
      <rect width="{w}" height="{H}" fill="#fff"/>
      <use href="#word" fill="#000"/>
    </mask>

{ends_mask}    <linearGradient id="body" gradientUnits="userSpaceOnUse" x1="0" y1="{BASELINE - 130}" x2="0" y2="{BASELINE + 35}">
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

    {glass_defs}
  </defs>

{outside_light}
  <!-- the glass: body tint, then the light seen through it -->
  {glass_body}
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
    layouts = {}

    def geometry_for(name):
        margin = {**BASE_VARIANT, **VARIANTS[name]}["margin"]
        if margin not in layouts:
            layouts[margin] = layout(a.font, margin)
        return layouts[margin]

    if a.options:
        opts = out / "options"
        opts.mkdir(exist_ok=True)
        for name in VARIANTS:
            for theme in THEMES:
                write(opts / f"logo-{theme}-{name}.svg", theme, name, geometry_for(name))
        (opts / "README.md").write_text(options_readme(list(VARIANTS)))
    else:
        for theme in THEMES:
            write(out / f"logo-{theme}.svg", theme, a.variant, geometry_for(a.variant))


if __name__ == "__main__":
    main()

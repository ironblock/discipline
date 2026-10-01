#!/usr/bin/env python3
"""Build logo/playground.html: the `refract` glass with sliders, to tweak on a phone.

build.py bakes the geometry (word outline, refraction map, rim-light segments) once; this
embeds that as JSON in playground.template.html, whose script redraws the SVG (a port of
build.svg for glass="refract") whenever a knob moves. The page shows it through <img>, as
GitHub does. Only what the baked maps fix (bezel width, glass thickness) is not a knob: the
two looks in lens.LOOKS are a switch.

usage: playground.py [out.html]      default logo/playground.html (not committed)
"""
import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))

import build
import lens

HERE = pathlib.Path(__file__).parent
OUTSIDE = {True: "full", "ends": "ends", False: "none"}


def data():
    geo = build.layout(build.DEFAULT_FONT, build.MARGIN)
    looks = {}
    for name, look in lens.LOOKS.items():
        maps = geo["maps"][name]
        bev, quant = build.bevel_paths(geo, maps["field"], None, look)
        looks[name] = dict(look=look, disp=maps["disp"], scale=maps["scale"], region=list(maps["region"]), bev=bev, quant=quant)
    presets = {}
    for name, variant in build.VARIANTS.items():
        merged = {**build.BASE_VARIANT, **variant}
        if merged["glass"] == "refract":
            presets[name] = dict(harmonics=merged["harmonics"], look=merged["look"], taper=merged["taper"],
                                 outside=OUTSIDE[merged["outside"]], gain=merged.get("gain", build.REFRACT_GAIN),
                                 desc=variant["desc"])
    consts = {k: getattr(build, k) for k in ("H", "BASELINE", "PAD", "CROP", "ENDS_FADE", "WAVELENGTH", "CROSS_OUTSIDE", "REFRACT_GAIN")}
    return dict(consts=consts, word=geo["d"], spans=geo["spans"], width=geo["width"], beamY=geo["beam_y"],
                xHeight=geo["x_height"], enters=geo["enters"], leaves=geo["leaves"], themes=build.THEMES,
                tapers=build.TAPERS, looks=looks, presets=presets, defaultPreset="refract-story-noise")


def main():
    out = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else HERE / "playground.html"
    template = (HERE / "playground.template.html").read_text()
    assert template.count("/*DATA*/null") == 1
    out.write_text(template.replace("/*DATA*/null", json.dumps(data(), default=float, separators=(",", ":"))))
    print(f"wrote {out} ({out.stat().st_size // 1024} KB)")


if __name__ == "__main__":
    main()

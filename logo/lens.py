"""Refraction and rim-light maps for the glass letters.

Follows https://kube.io/blog/liquid-glass-css-svg/ : a convex bezel profile, Snell's
law at the top surface, displacement along the edge normal, all baked into PNGs that a
filter reads with feImage. There the shape is a rounded rectangle with a closed-form
distance; here it is each glyph, so distance and normal come from a distance transform
of the rasterised outlines.

Lengths are in the SVG's user units. Two maps, both opaque RGB:
  disp: R, G = displacement along x, y (128 = none), for feDisplacementMap
  spec: R = light from the top-left, G = light from the bottom-right, B = edge band
"""
import base64
import io

import numpy as np
from fontTools.pens.basePen import BasePen
from PIL import Image, ImageDraw
from scipy import ndimage

RES = 2           # raster pixels per unit while computing
OUT_RES = 1       # pixels per unit in the delivered PNGs (they are smooth; feImage interpolates)
IOR = 1.5
# A look is a bezel (the curved edge: width and glass thickness, which sets how far light is
# thrown) plus how the rim light spreads. Narrow and bright reads as carved; wide and soft as liquid.
LOOKS = {
    "carved": dict(bezel=8.0, thickness=11.0, spec_width=5.0, spec_power=1.0, band_width=7.0),
    "liquid": dict(bezel=14.0, thickness=20.0, spec_width=7.0, spec_power=1.0, band_width=9.0),
}
LIGHT = (-0.7071, -0.7071)  # unit vector toward the light, image coordinates (top-left)


class _Flatten(BasePen):
    def __init__(self, glyphset, xform, steps=14):
        super().__init__(glyphset)
        self.xform, self.steps, self.contours, self._c = xform, steps, [], None

    def _moveTo(self, p):
        self._c = [self.xform(p)]
        self.contours.append(self._c)

    def _lineTo(self, p):
        self._c.append(self.xform(p))

    def _curveToOne(self, p1, p2, p3):
        p0 = self._getCurrentPoint()
        for i in range(1, self.steps + 1):
            t = i / self.steps
            u = 1 - t
            self._c.append(self.xform(tuple(u**3 * a + 3 * u * u * t * b + 3 * u * t * t * c + t**3 * d
                                            for a, b, c, d in zip(p0, p1, p2, p3))))

    def _qCurveToOne(self, p1, p2):
        p0 = self._getCurrentPoint()
        for i in range(1, self.steps + 1):
            t = i / self.steps
            u = 1 - t
            self._c.append(self.xform(tuple(u * u * a + 2 * u * t * b + t * t * c
                                            for a, b, c in zip(p0, p1, p2))))

    def _closePath(self):
        pass


def glyph_contours(glyphset, name, x, baseline, scale, steps=14):
    """Flattened outline of one glyph as polygons in user units (y down)."""
    pen = _Flatten(glyphset, lambda p: (x + p[0] * scale, baseline - p[1] * scale), steps)
    glyphset[name].draw(pen)
    return pen.contours


def _raster(contours, region):
    """Coverage mask at RES px/unit. Contours are combined even-odd, so counters are holes."""
    x0, y0, w, h = region
    mask = np.zeros((h * RES, w * RES), bool)
    for c in contours:
        im = Image.new("L", mask.shape[::-1], 0)
        ImageDraw.Draw(im).polygon([((px - x0) * RES, (py - y0) * RES) for px, py in c], fill=255)
        mask ^= np.asarray(im) > 0
    return mask


def _bezel_shift(d, bezel, thickness):
    """How far a vertical ray is thrown sideways at distance d inside the edge. A convex
    squircle bezel: steepest at the edge, flat by `bezel`. Snell's law at the top surface."""
    t = np.clip(d / bezel, 1e-3, 1.0)
    rise = (1 - t) ** 3 * (1 - (1 - t) ** 4) ** -0.75           # dh/dt of h = (1-(1-t)^4)^(1/4)
    alpha = np.arctan(thickness / bezel * rise)                  # surface tilt from horizontal
    bend = alpha - np.arcsin(np.sin(alpha) / IOR)                # deviation from the vertical
    return np.where(d < bezel, thickness * np.tan(bend), 0.0)


def _downsample(a):
    k = RES // OUT_RES
    h, w = a.shape[0] // k * k, a.shape[1] // k * k
    return a[:h, :w].reshape(h // k, k, w // k, k, *a.shape[2:]).mean(axis=(1, 3))


def png_constant(rgb, size=8):
    """A flat opaque PNG as a data URI (for the feImage diagnostic)."""
    return _png(np.full((size, size, 3), rgb, float))


def _png(rgb):
    buf = io.BytesIO()
    Image.fromarray(np.clip(np.rint(rgb), 0, 255).astype(np.uint8), "RGB").save(buf, "PNG", optimize=True)
    return "data:image/png;base64," + base64.b64encode(buf.getvalue()).decode()


def maps(contours, region, look):
    """region = (x, y, w, h) in whole user units; look is one of LOOKS. Returns data URIs and
    the displacement scale."""
    mask = _raster(contours, region)
    d = np.maximum(ndimage.distance_transform_edt(mask) - 0.5, 0) / RES     # units from the edge, inside
    gy, gx = np.gradient(ndimage.gaussian_filter(d, 1.0))                   # points inward
    norm = np.hypot(gx, gy)
    ux, uy = np.divide(gx, norm, out=np.zeros_like(gx), where=norm > 1e-6), np.divide(gy, norm, out=np.zeros_like(gy), where=norm > 1e-6)

    shift = _bezel_shift(d, look["bezel"], look["thickness"]) * mask
    peak = shift.max()
    disp = np.stack([127.5 + 127.5 * shift * ux / peak, 127.5 + 127.5 * shift * uy / peak, np.full_like(d, 128)], -1)
    disp[~mask] = 128

    def smoothstep(a, b, x):
        t = np.clip((x - a) / (b - a), 0, 1)
        return t * t * (3 - 2 * t)

    toward = -(ux * LIGHT[0] + uy * LIGHT[1])        # outward normal . light direction
    fall = (1 - smoothstep(0, look["spec_width"], d)) * mask
    lit = np.clip(toward, 0, 1) ** look["spec_power"] * fall
    shade = np.clip(-toward, 0, 1) ** look["spec_power"] * fall
    band = (1 - smoothstep(0, look["band_width"], d)) * mask
    spec = np.stack([lit, shade, band], -1) * 255

    field = dict(region=region, dx=shift * ux, dy=shift * uy, ux=ux, uy=uy, inside=mask.astype(float))
    return dict(disp=_png(_downsample(disp)), spec=_png(_downsample(spec)), scale=2 * peak, region=region, field=field)


def sample(field, name, pts):
    """Bilinear lookup of a field array at points (N, 2) in user units."""
    x0, y0 = field["region"][:2]
    pts = np.asarray(pts, float)
    coords = np.array([(pts[:, 1] - y0) * RES - 0.5, (pts[:, 0] - x0) * RES - 0.5])
    return ndimage.map_coordinates(field[name], coords, order=1, mode="nearest")


def refract(field, pts, gain=1.0):
    """Where content at pts is seen through the glass. The filter shows, at each pixel, the source
    at pixel + displacement (inward); so content appears displaced the other way, toward the edge."""
    pts = np.asarray(pts, float)
    return pts - gain * np.stack([sample(field, "dx", pts), sample(field, "dy", pts)], 1)


def inside(field, pts):
    return sample(field, "inside", pts) > 0.5

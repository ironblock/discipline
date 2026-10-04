#!/usr/bin/env python3
"""#373's vision cell: the image, from a fixed seed. Stdlib only, deterministic: the same seed gives the same bytes.

One synthetic PNG on a plain background with one planted detail: a four-character token drawn from an alphabet with
no look-alike glyphs (no 0/O, 1/I/L, 2/Z, 5/S, 6/G, 8/B, U/V, D), rendered large from a 5x7 bitmap font in one colour
at a seeded position. Usage: make_image.py OUT_DIR -> OUT_DIR/image.png and OUT_DIR/token.sha256.
With --token it prints the token, which only the recompute's comparison uses."""
import hashlib, pathlib, random, struct, sys, zlib

SEED_PHRASE = "discipline #373 vision cell, 2026-10-04"
SEED = int(hashlib.sha256(SEED_PHRASE.encode()).hexdigest()[:16], 16)
ALPHABET = "ACEFHKMNPRTWXY347"
W, H, SCALE, GAP = 640, 360, 12, 2  # canvas, font-pixel size, glyph gap in font pixels
BG, FG = (255, 255, 255), (20, 20, 120)
FONT = {  # 5x7, rows top to bottom, '#' set
    "A": [".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"],
    "C": [".####", "#....", "#....", "#....", "#....", "#....", ".####"],
    "E": ["#####", "#....", "#....", "####.", "#....", "#....", "#####"],
    "F": ["#####", "#....", "#....", "####.", "#....", "#....", "#...."],
    "H": ["#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"],
    "K": ["#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#"],
    "M": ["#...#", "##.##", "#.#.#", "#.#.#", "#...#", "#...#", "#...#"],
    "N": ["#...#", "##..#", "#.#.#", "#..##", "#...#", "#...#", "#...#"],
    "P": ["####.", "#...#", "#...#", "####.", "#....", "#....", "#...."],
    "R": ["####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#"],
    "T": ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#.."],
    "W": ["#...#", "#...#", "#...#", "#.#.#", "#.#.#", "##.##", "#...#"],
    "X": ["#...#", "#...#", ".#.#.", "..#..", ".#.#.", "#...#", "#...#"],
    "Y": ["#...#", "#...#", ".#.#.", "..#..", "..#..", "..#..", "..#.."],
    "3": ["####.", "....#", "....#", ".###.", "....#", "....#", "####."],
    "4": ["#...#", "#...#", "#...#", "#####", "....#", "....#", "....#"],
    "7": ["#####", "....#", "...#.", "..#..", ".#...", ".#...", ".#..."],
}
assert set(FONT) == set(ALPHABET)


def token_and_origin():
    r = random.Random(SEED)
    token = "".join(r.choice(ALPHABET) for _ in range(4))
    tw, th = (4 * 5 + 3 * GAP) * SCALE, 7 * SCALE
    return token, (r.randrange(SCALE, W - tw - SCALE), r.randrange(SCALE, H - th - SCALE))


def png(pixels):
    raw = b"".join(b"\x00" + bytes(c for px in row for c in px) for row in pixels)
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def render():
    token, (x0, y0) = token_and_origin()
    pixels = [[BG] * W for _ in range(H)]
    for i, ch in enumerate(token):
        gx = x0 + i * (5 + GAP) * SCALE
        for ry, row in enumerate(FONT[ch]):
            for rx, bit in enumerate(row):
                if bit == "#":
                    for y in range(y0 + ry * SCALE, y0 + (ry + 1) * SCALE):
                        for x in range(gx + rx * SCALE, gx + (rx + 1) * SCALE):
                            pixels[y][x] = FG
    return token, png(pixels)


if __name__ == "__main__":
    token, data = render()
    if sys.argv[1:] == ["--token"]:
        print(token); sys.exit(0)
    out = pathlib.Path(sys.argv[1]); out.mkdir(parents=True, exist_ok=True)
    (out / "image.png").write_bytes(data)
    (out / "token.sha256").write_text(hashlib.sha256(token.encode()).hexdigest() + "\n")
    print(hashlib.sha256(data).hexdigest())

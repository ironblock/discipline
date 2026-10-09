/**
 * A PNG, written here: the authored sessions' screenshots (#372) are made by
 * this function from their own few numbers, so no image is committed and
 * none has a provenance but this file. Truecolour, 8 bits, no filter, the
 * image data in stored (uncompressed) deflate blocks: valid PNG any decoder
 * reads, and small enough at the sizes a fixture needs.
 */

const CRC = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n += 1) {
    let c = n;
    for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c >>> 0;
  }
  return table;
})();

function crc32(bytes: Uint8Array): number {
  let c = 0xffffffff;
  for (const b of bytes) c = CRC[(c ^ b) & 0xff]! ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function adler32(bytes: Uint8Array): number {
  let a = 1;
  let b = 0;
  for (const x of bytes) {
    a = (a + x) % 65521;
    b = (b + a) % 65521;
  }
  return ((b << 16) | a) >>> 0;
}

/** The eight bytes every PNG begins with. */
const SIGNATURE = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a] as const;

/** Whether BYTES begin as a PNG does: the media type is the bytes', never a name's (`diet/src/drive/attach.rs`). */
export function isPng(bytes: Uint8Array): boolean {
  return SIGNATURE.every((b, i) => bytes[i] === b);
}

const u32 = (n: number) => [(n >>> 24) & 0xff, (n >>> 16) & 0xff, (n >>> 8) & 0xff, n & 0xff];

function chunk(type: string, data: Uint8Array): number[] {
  const typed = new Uint8Array([...type].map((c) => c.charCodeAt(0)).concat([...data]));
  return [...u32(data.length), ...typed, ...u32(crc32(typed))];
}

/** A WIDTH × HEIGHT image whose pixel at (x, y) is PIXEL's [r, g, b]. */
export function png(width: number, height: number, pixel: (x: number, y: number) => readonly [number, number, number]): Uint8Array {
  const raw: number[] = [];
  for (let y = 0; y < height; y += 1) {
    raw.push(0);
    for (let x = 0; x < width; x += 1) raw.push(...pixel(x, y));
  }
  const data = new Uint8Array(raw);
  // zlib: a header, stored blocks of at most 65,535 bytes, and the Adler-32 of the data.
  const z: number[] = [0x78, 0x01];
  for (let at = 0; at < data.length || at === 0; at += 65535) {
    const block = data.subarray(at, at + 65535);
    const last = at + 65535 >= data.length ? 1 : 0;
    z.push(last, block.length & 0xff, block.length >>> 8, ~block.length & 0xff, (~block.length >>> 8) & 0xff, ...block);
    if (last) break;
  }
  z.push(...u32(adler32(data)));
  const header = new Uint8Array([...u32(width), ...u32(height), 8, 2, 0, 0, 0]);
  return new Uint8Array([...SIGNATURE, ...chunk('IHDR', header), ...chunk('IDAT', new Uint8Array(z)), ...chunk('IEND', new Uint8Array())]);
}

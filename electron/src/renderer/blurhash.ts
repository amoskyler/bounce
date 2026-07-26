/**
 * BlurHash, encode and decode.
 *
 * A BlurHash is a very short string — around thirty characters — describing an
 * image's low-frequency content. It travels inside the message, so a recipient
 * can show a recognisable blur of a picture the instant the message lands,
 * while the megabytes of the picture itself are still being fetched a chunk at
 * a time. Without it an image arrives as a grey rectangle for however long the
 * transfer takes.
 *
 * Implemented here rather than pulled from npm because it is a hundred lines
 * of arithmetic against a fixed published format, and because the renderer
 * runs under a content security policy that forbids fetching anything.
 *
 * The parameters match the Go client exactly — 4×4 components, encoded from a
 * copy scaled so its longest side is about 32 pixels (`ui/pending_message_attachments.go`).
 * A hash produced here has to be one that build can decode, and vice versa.
 *
 * Reference: <https://github.com/woltapp/blurhash/blob/master/Algorithm.md>
 */

/** The alphabet BlurHash uses for its base-83 integers. */
const DIGITS = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz#$%*+,-.:;=?@[]^_{|}~';

/** Components, matching the Go client. Changing this changes the wire format. */
export const COMPONENTS_X = 4;
export const COMPONENTS_Y = 4;

/** Longest side of the copy the hash is computed from. */
const ENCODE_SIZE = 32;

function encode83(value: number, length: number): string {
  let out = '';
  for (let index = 1; index <= length; index += 1) {
    const digit = Math.floor(value / 83 ** (length - index)) % 83;
    out += DIGITS[digit];
  }
  return out;
}

function decode83(value: string): number {
  let out = 0;
  for (const character of value) {
    const digit = DIGITS.indexOf(character);
    if (digit === -1) throw new Error('invalid blurhash character');
    out = out * 83 + digit;
  }
  return out;
}

/** sRGB byte to linear light. */
function toLinear(value: number): number {
  const v = value / 255;
  return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
}

/**
 * Linear light back to an sRGB byte.
 *
 * `floor(x + 0.5)`, not `round(x + 0.5)` — the reference implementations write
 * this as a C cast, which truncates, so rounding as well shifts every value up
 * by one at the halfway point.
 */
function toSrgb(value: number): number {
  const v = Math.max(0, Math.min(1, value));
  return Math.floor(
    (v <= 0.0031308 ? v * 12.92 : 1.055 * v ** (1 / 2.4) - 0.055) * 255 + 0.5,
  );
}

function signPow(value: number, exponent: number): number {
  return Math.sign(value) * Math.abs(value) ** exponent;
}

/**
 * Encode RGBA pixel data as a BlurHash.
 *
 * `pixels` is the four-bytes-per-pixel buffer a canvas produces.
 */
export function encodeBlurHash(
  pixels: Uint8ClampedArray,
  width: number,
  height: number,
): string {
  if (width < 1 || height < 1 || pixels.length !== 4 * width * height) {
    throw new Error('blurhash: pixel data does not match the given size');
  }

  const factors: Array<[number, number, number]> = [];
  for (let y = 0; y < COMPONENTS_Y; y += 1) {
    for (let x = 0; x < COMPONENTS_X; x += 1) {
      // The DC component (0,0) has a flat basis and so a different scale.
      const normalisation = x === 0 && y === 0 ? 1 : 2;
      let r = 0;
      let g = 0;
      let b = 0;

      for (let px = 0; px < width; px += 1) {
        for (let py = 0; py < height; py += 1) {
          const basis =
            normalisation *
            Math.cos((Math.PI * x * px) / width) *
            Math.cos((Math.PI * y * py) / height);
          const offset = 4 * px + py * 4 * width;
          r += basis * toLinear(pixels[offset]);
          g += basis * toLinear(pixels[offset + 1]);
          b += basis * toLinear(pixels[offset + 2]);
        }
      }

      const scale = 1 / (width * height);
      factors.push([r * scale, g * scale, b * scale]);
    }
  }

  const [dc, ...ac] = factors;

  let hash = encode83(COMPONENTS_X - 1 + (COMPONENTS_Y - 1) * 9, 1);

  // Every AC component is stored relative to the largest one, so the quantised
  // values use the full range whatever the image's contrast.
  const maximum = ac.length ? Math.max(...ac.flat().map(Math.abs)) : 0;
  const quantised = ac.length
    ? Math.max(0, Math.min(82, Math.floor(maximum * 166 - 0.5)))
    : 0;
  const maximumValue = ac.length ? (quantised + 1) / 166 : 1;
  hash += encode83(quantised, 1);

  hash += encode83(
    (toSrgb(dc[0]) << 16) + (toSrgb(dc[1]) << 8) + toSrgb(dc[2]),
    4,
  );

  for (const [r, g, b] of ac) {
    const quantise = (value: number) =>
      Math.max(0, Math.min(18, Math.floor(signPow(value / maximumValue, 0.5) * 9 + 9.5)));
    hash += encode83(quantise(r) * 19 * 19 + quantise(g) * 19 + quantise(b), 2);
  }

  return hash;
}

/** Decoded RGBA pixels, ready for a canvas. */
export type DecodedBlurHash = {
  pixels: Uint8ClampedArray;
  width: number;
  height: number;
};

/**
 * Decode a BlurHash into RGBA pixels.
 *
 * `width` and `height` are the size to render at, not the source image's — the
 * hash carries no resolution, so a handful of pixels is plenty and cheaper.
 */
export function decodeBlurHash(hash: string, width = 32, height = 32): DecodedBlurHash {
  if (hash.length < 6) throw new Error('blurhash: too short');

  const sizeFlag = decode83(hash[0]);
  const componentsX = (sizeFlag % 9) + 1;
  const componentsY = Math.floor(sizeFlag / 9) + 1;

  if (hash.length !== 4 + 2 * componentsX * componentsY) {
    throw new Error('blurhash: length does not match its component count');
  }

  const maximumValue = (decode83(hash[1]) + 1) / 166;

  const colours: Array<[number, number, number]> = [];
  for (let index = 0; index < componentsX * componentsY; index += 1) {
    if (index === 0) {
      const value = decode83(hash.slice(2, 6));
      colours.push([
        toLinear(value >> 16),
        toLinear((value >> 8) & 255),
        toLinear(value & 255),
      ]);
    } else {
      const value = decode83(hash.slice(4 + index * 2, 6 + index * 2));
      const quantR = Math.floor(value / (19 * 19));
      const quantG = Math.floor(value / 19) % 19;
      const quantB = value % 19;
      colours.push([
        signPow((quantR - 9) / 9, 2) * maximumValue,
        signPow((quantG - 9) / 9, 2) * maximumValue,
        signPow((quantB - 9) / 9, 2) * maximumValue,
      ]);
    }
  }

  const pixels = new Uint8ClampedArray(width * height * 4);
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      let r = 0;
      let g = 0;
      let b = 0;

      for (let j = 0; j < componentsY; j += 1) {
        for (let i = 0; i < componentsX; i += 1) {
          const basis =
            Math.cos((Math.PI * x * i) / width) * Math.cos((Math.PI * y * j) / height);
          const colour = colours[i + j * componentsX];
          r += colour[0] * basis;
          g += colour[1] * basis;
          b += colour[2] * basis;
        }
      }

      const offset = 4 * (x + y * width);
      pixels[offset] = toSrgb(r);
      pixels[offset + 1] = toSrgb(g);
      pixels[offset + 2] = toSrgb(b);
      pixels[offset + 3] = 255;
    }
  }

  return { pixels, width, height };
}

/**
 * A BlurHash as a `data:` URL, for use as a CSS background or an `<img>` src.
 *
 * Rendered small and stretched by the browser, which is both faster and
 * closer to the intended look than decoding at full size.
 */
export function blurHashToDataUrl(hash: string, width = 32, height = 32): string | null {
  try {
    const { pixels } = decodeBlurHash(hash, width, height);
    const canvas = document.createElement('canvas');
    canvas.width = width;
    canvas.height = height;

    const context = canvas.getContext('2d');
    if (!context) return null;

    const image = context.createImageData(width, height);
    image.data.set(pixels);
    context.putImageData(image, 0, 0);

    return canvas.toDataURL();
  } catch {
    // A malformed hash is somebody else's bad encoder, not a reason to lose
    // the message it arrived with.
    return null;
  }
}

/**
 * Compute a BlurHash for an image that has already been decoded by the browser.
 *
 * The image is drawn into a small canvas first: the encode is O(pixels ×
 * components), so hashing a twelve megapixel photo at full size would block
 * the renderer for seconds to produce the same thirty characters. The Go
 * client scales to the same size for the same reason.
 */
export function blurHashFromImage(image: HTMLImageElement): string | null {
  const longest = Math.max(image.naturalWidth, image.naturalHeight);
  if (longest === 0) return null;

  const scale = longest > ENCODE_SIZE ? ENCODE_SIZE / longest : 1;
  const width = Math.max(1, Math.round(image.naturalWidth * scale));
  const height = Math.max(1, Math.round(image.naturalHeight * scale));

  try {
    const canvas = document.createElement('canvas');
    canvas.width = width;
    canvas.height = height;

    const context = canvas.getContext('2d', { willReadFrequently: true });
    if (!context) return null;

    context.drawImage(image, 0, 0, width, height);
    return encodeBlurHash(context.getImageData(0, 0, width, height).data, width, height);
  } catch {
    // A canvas that refuses to be read is not worth failing a send over.
    return null;
  }
}

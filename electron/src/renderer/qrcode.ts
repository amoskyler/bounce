/**
 * QR encoding, in the renderer.
 *
 * The Fyne client shows the pairing code as a QR so the person standing next to
 * you can point a phone at it (`ui/add_user.go:54`). Doing the same here means
 * encoding one in the renderer, and the renderer runs under a content security
 * policy with no network and no remote script: there is no CDN to pull a
 * library from, and adding the first runtime dependency to a client whose whole
 * pitch is that it talks to nobody is not a trade worth making for one square.
 *
 * So this is a QR encoder, in about three hundred lines. It is a port of the
 * `piglig/go-qr` encoder the Go client already depends on (itself a port of
 * Nayuki's reference implementation), kept faithful down to the mask-penalty
 * arithmetic so that both clients draw the same square for the same code — the
 * tests compare module for module against its output.
 *
 * Byte mode only. A pairing code is `<onion address>:<hex secret>`, which is
 * lowercase, so the numeric and alphanumeric modes could never apply to it, and
 * a mode that can never be selected is a mode that can never be tested.
 */

/** Error correction levels, in the order the format-bit table indexes them. */
type Level = 0 | 1 | 2 | 3;

const MEDIUM: Level = 1;

/** The format bits each level is written with. Not the same order as `Level`. */
const LEVEL_FORMAT_BITS = [1, 0, 3, 2];

const MIN_VERSION = 1;
const MAX_VERSION = 40;

/** Error correction codewords per block, by level then version. */
const ECC_CODEWORDS_PER_BLOCK: ReadonlyArray<ReadonlyArray<number>> = [
  // 0 is padding: there is no version 0.
  // 0   1   2   3   4   5   6   7   8   9  10  11  12  13  14  15  16  17  18  19  20  21  22  23  24  25  26  27  28  29  30  31  32  33  34  35  36  37  38  39  40
  [-1, 7, 10, 15, 20, 26, 18, 20, 24, 30, 18, 20, 24, 26, 30, 22, 24, 28, 30, 28, 28, 28, 28, 30, 30, 26, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
  [-1, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26, 30, 22, 22, 24, 24, 28, 28, 26, 26, 26, 26, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28],
  [-1, 13, 22, 18, 26, 18, 24, 18, 22, 20, 24, 28, 26, 24, 20, 30, 24, 28, 28, 26, 30, 28, 30, 30, 30, 30, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
  [-1, 17, 28, 22, 16, 22, 28, 26, 26, 24, 28, 24, 28, 22, 24, 24, 30, 28, 28, 26, 28, 30, 24, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
];

/** Error correction blocks, by level then version. */
const ECC_BLOCKS: ReadonlyArray<ReadonlyArray<number>> = [
  // 0  1  2  3  4  5  6  7  8  9 10  11  12  13  14  15  16  17  18  19  20  21  22  23  24  25  26  27  28  29  30  31  32  33  34  35  36  37  38  39  40
  [-1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 4, 4, 4, 4, 4, 6, 6, 6, 6, 7, 8, 8, 9, 9, 10, 12, 12, 12, 13, 14, 15, 16, 17, 18, 19, 19, 20, 21, 22, 24, 25],
  [-1, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5, 5, 8, 9, 9, 10, 10, 11, 13, 14, 16, 17, 17, 18, 20, 21, 23, 25, 26, 28, 29, 31, 33, 35, 37, 38, 40, 43, 45, 47, 49],
  [-1, 1, 1, 2, 2, 4, 4, 6, 6, 8, 8, 8, 10, 12, 16, 12, 17, 16, 18, 21, 20, 23, 23, 25, 27, 29, 34, 34, 35, 38, 40, 43, 45, 48, 51, 53, 56, 59, 62, 65, 68],
  [-1, 1, 1, 2, 4, 4, 4, 5, 6, 8, 8, 11, 11, 16, 16, 18, 16, 19, 21, 25, 25, 25, 34, 30, 32, 35, 37, 40, 42, 45, 48, 51, 54, 57, 60, 63, 66, 70, 74, 77, 81],
];

/** Mask penalty weights, from the specification. */
const PENALTY_N1 = 3;
const PENALTY_N2 = 3;
const PENALTY_N3 = 40;
const PENALTY_N4 = 10;

/** A finished symbol. The quiet zone is the renderer's business, not this. */
export type QrMatrix = {
  /** Side length in modules. */
  size: number;
  /** Row-major, `modules[y][x]`; true is dark. */
  modules: boolean[][];
};

/**
 * Encode text as a QR symbol.
 *
 * Starts at error correction level M — what the Go client asks for — and, once
 * the version is settled, raises the level as far as the same square will hold,
 * which is free redundancy for a code being read off somebody's screen at an
 * angle.
 *
 * Throws when the text will not fit in a version 40 symbol, which for a pairing
 * code cannot happen: 89 characters lands in version 6.
 */
export function encodeQr(text: string): QrMatrix {
  const data = new TextEncoder().encode(text);

  let version = MIN_VERSION;
  let level: Level = MEDIUM;
  for (;;) {
    if (usedBits(data.length, version) <= dataCodewords(version, level) * 8) break;
    version += 1;
    if (version > MAX_VERSION) {
      throw new Error(`${data.length} bytes is too long for a QR code`);
    }
  }

  const used = usedBits(data.length, version);
  for (const candidate of [1, 2, 3] as Level[]) {
    if (used <= dataCodewords(version, candidate) * 8) level = candidate;
  }

  const grid = blankGrid(version, level);
  drawFunctionPatterns(grid);
  drawCodewords(grid, addEccAndInterleave(grid, codewords(data, version, level)));

  const mask = chooseMask(grid);
  applyMask(grid, mask);
  drawFormatBits(grid, mask);

  return { size: grid.size, modules: grid.modules };
}

/* --------------------------------------------------------------------------
 * Capacity arithmetic
 * -------------------------------------------------------------------------- */

/** Bits a byte-mode segment of `length` bytes occupies at this version. */
function usedBits(length: number, version: number): number {
  // Mode indicator, then the character count — eight bits up to version 9,
  // sixteen from version 10, where the counts stop fitting.
  return 4 + (version < 10 ? 8 : 16) + length * 8;
}

/** Modules available for data and error correction, before either is placed. */
function rawDataModules(version: number): number {
  const size = version * 4 + 17;
  let result = size * size;

  // The three 8x8 finder regions, including their separators.
  result -= 8 * 8 * 3;
  // The two timing patterns, and the dark module beside the lower-left finder.
  result -= 15 * 2 + 1;
  // The timing patterns' continuation between the finders.
  result -= (size - 16) * 2;

  if (version >= 2) {
    const alignments = Math.floor(version / 7) + 2;
    result -= (alignments - 1) * (alignments - 1) * 25;
    result -= (alignments - 2) * 2 * 20;
    if (version >= 7) result -= 6 * 3 * 2;
  }

  return result;
}

function dataCodewords(version: number, level: Level): number {
  return (
    Math.floor(rawDataModules(version) / 8) -
    ECC_CODEWORDS_PER_BLOCK[level][version] * ECC_BLOCKS[level][version]
  );
}

/* --------------------------------------------------------------------------
 * The bit stream
 * -------------------------------------------------------------------------- */

/** The data codewords for a payload: header, bytes, terminator, padding. */
function codewords(data: Uint8Array, version: number, level: Level): Uint8Array {
  const capacity = dataCodewords(version, level) * 8;
  const bits: number[] = [];

  const append = (value: number, count: number) => {
    for (let i = count - 1; i >= 0; i -= 1) bits.push((value >>> i) & 1);
  };

  append(0b0100, 4);
  append(data.length, version < 10 ? 8 : 16);
  for (const byte of data) append(byte, 8);

  // A terminator of up to four zero bits, then zeroes to the byte boundary.
  append(0, Math.min(4, capacity - bits.length));
  append(0, (8 - (bits.length % 8)) % 8);

  // The two pad bytes alternate, which is what the specification asks for; any
  // fixed filler would decode, but not every reader is forgiving.
  for (let pad = 0xec; bits.length < capacity; pad ^= 0xec ^ 0x11) append(pad, 8);

  const result = new Uint8Array(bits.length / 8);
  for (let i = 0; i < bits.length; i += 1) {
    result[i >>> 3] |= bits[i] << (7 - (i & 7));
  }
  return result;
}

/* --------------------------------------------------------------------------
 * Reed-Solomon over GF(2^8), with the QR field polynomial 0x11d
 * -------------------------------------------------------------------------- */

const GF_EXP = new Uint8Array(512);
const GF_LOG = new Uint8Array(256);

(() => {
  let x = 1;
  for (let i = 0; i < 255; i += 1) {
    GF_EXP[i] = x;
    GF_LOG[x] = i;
    // Russian peasant multiplication by the generator, 0x02.
    x = ((x << 1) ^ ((x >>> 7) * 0x11d)) & 0xff;
  }
  // Doubling the table lets a product skip the modulo on the exponent.
  for (let i = 255; i < 512; i += 1) GF_EXP[i] = GF_EXP[i - 255];
})();

function gfMul(a: number, b: number): number {
  if (a === 0 || b === 0) return 0;
  return GF_EXP[GF_LOG[a] + GF_LOG[b]];
}

/** The generator polynomial of the given degree, highest term implied. */
function divisor(degree: number): Uint8Array {
  const result = new Uint8Array(degree);
  result[degree - 1] = 1;

  let root = 1;
  for (let i = 0; i < degree; i += 1) {
    for (let j = 0; j < degree; j += 1) {
      result[j] = gfMul(result[j], root);
      if (j + 1 < degree) result[j] ^= result[j + 1];
    }
    root = gfMul(root, 0x02);
  }
  return result;
}

/** The remainder of `data` divided by the generator: the ECC codewords. */
function remainder(data: Uint8Array, generator: Uint8Array): Uint8Array {
  const result = new Uint8Array(generator.length);
  for (const byte of data) {
    const factor = byte ^ result[0];
    result.copyWithin(0, 1);
    result[result.length - 1] = 0;
    for (let i = 0; i < result.length; i += 1) result[i] ^= gfMul(generator[i], factor);
  }
  return result;
}

/**
 * Split the data into blocks, add each block's ECC, and interleave.
 *
 * Interleaving is what makes the error correction worth having: a thumb over
 * one corner of the code damages a few codewords of every block rather than
 * destroying one block outright.
 */
function addEccAndInterleave(grid: Grid, data: Uint8Array): Uint8Array {
  const blockCount = ECC_BLOCKS[grid.level][grid.version];
  const eccLength = ECC_CODEWORDS_PER_BLOCK[grid.level][grid.version];
  const rawCodewords = Math.floor(rawDataModules(grid.version) / 8);

  const shortBlocks = blockCount - (rawCodewords % blockCount);
  const shortLength = Math.floor(rawCodewords / blockCount);

  const generator = divisor(eccLength);
  const blocks: Uint8Array[] = [];

  for (let i = 0, offset = 0; i < blockCount; i += 1) {
    const dataLength = shortLength - eccLength + (i < shortBlocks ? 0 : 1);
    const chunk = data.subarray(offset, offset + dataLength);
    offset += dataLength;

    // Every block is padded to the longest length so the interleave below can
    // walk them in step; the extra byte of a short block is skipped there.
    const block = new Uint8Array(shortLength + 1);
    block.set(chunk);
    block.set(remainder(chunk, generator), block.length - eccLength);
    blocks.push(block);
  }

  const result = new Uint8Array(rawCodewords);
  for (let i = 0, k = 0; i < blocks[0].length; i += 1) {
    for (let j = 0; j < blocks.length; j += 1) {
      if (i !== shortLength - eccLength || j >= shortBlocks) {
        result[k] = blocks[j][i];
        k += 1;
      }
    }
  }
  return result;
}

/* --------------------------------------------------------------------------
 * The grid
 * -------------------------------------------------------------------------- */

type Grid = {
  version: number;
  level: Level;
  size: number;
  modules: boolean[][];
  /** True where a module belongs to a pattern and so is never masked. */
  isFunction: boolean[][];
};

function blankGrid(version: number, level: Level): Grid {
  const size = version * 4 + 17;
  const rows = () => Array.from({ length: size }, () => new Array<boolean>(size).fill(false));
  return { version, level, size, modules: rows(), isFunction: rows() };
}

function setFunction(grid: Grid, x: number, y: number, dark: boolean): void {
  grid.modules[y][x] = dark;
  grid.isFunction[y][x] = true;
}

function drawFunctionPatterns(grid: Grid): void {
  for (let i = 0; i < grid.size; i += 1) {
    setFunction(grid, 6, i, i % 2 === 0);
    setFunction(grid, i, 6, i % 2 === 0);
  }

  drawFinder(grid, 3, 3);
  drawFinder(grid, grid.size - 4, 3);
  drawFinder(grid, 3, grid.size - 4);

  const positions = alignmentPositions(grid.version, grid.size);
  const last = positions.length - 1;
  for (let i = 0; i <= last; i += 1) {
    for (let j = 0; j <= last; j += 1) {
      // The three corners are where the finders already are.
      const corner = (i === 0 && j === 0) || (i === 0 && j === last) || (i === last && j === 0);
      if (!corner) drawAlignment(grid, positions[i], positions[j]);
    }
  }

  // Reserved now with mask 0's bits, overwritten once the mask is chosen.
  drawFormatBits(grid, 0);
  drawVersionBits(grid);
}

function drawFinder(grid: Grid, x: number, y: number): void {
  for (let dy = -4; dy <= 4; dy += 1) {
    for (let dx = -4; dx <= 4; dx += 1) {
      const distance = Math.max(Math.abs(dx), Math.abs(dy));
      const xx = x + dx;
      const yy = y + dy;
      if (xx >= 0 && xx < grid.size && yy >= 0 && yy < grid.size) {
        setFunction(grid, xx, yy, distance !== 2 && distance !== 4);
      }
    }
  }
}

function drawAlignment(grid: Grid, x: number, y: number): void {
  for (let dy = -2; dy <= 2; dy += 1) {
    for (let dx = -2; dx <= 2; dx += 1) {
      setFunction(grid, x + dx, y + dy, Math.max(Math.abs(dx), Math.abs(dy)) !== 1);
    }
  }
}

function alignmentPositions(version: number, size: number): number[] {
  if (version === 1) return [];

  const count = Math.floor(version / 7) + 2;
  // Version 32 is the one the general formula does not describe.
  const step =
    version === 32 ? 26 : Math.floor((version * 4 + count * 2 + 1) / (count * 2 - 2)) * 2;

  const result = new Array<number>(count);
  result[0] = 6;
  for (let i = count - 1, position = size - 7; i >= 1; i -= 1, position -= step) {
    result[i] = position;
  }
  return result;
}

function drawFormatBits(grid: Grid, mask: number): void {
  const data = (LEVEL_FORMAT_BITS[grid.level] << 3) | mask;

  // BCH(15, 5), then the mask that stops an all-zero format from reading as a
  // valid one.
  let rem = data;
  for (let i = 0; i < 10; i += 1) rem = (rem << 1) ^ ((rem >>> 9) * 0x537);
  const bits = ((data << 10) | rem) ^ 0x5412;

  for (let i = 0; i <= 5; i += 1) setFunction(grid, 8, i, bit(bits, i));
  setFunction(grid, 8, 7, bit(bits, 6));
  setFunction(grid, 8, 8, bit(bits, 7));
  setFunction(grid, 7, 8, bit(bits, 8));
  for (let i = 9; i < 15; i += 1) setFunction(grid, 14 - i, 8, bit(bits, i));

  // The second copy, so a damaged corner does not cost the format.
  for (let i = 0; i < 8; i += 1) setFunction(grid, grid.size - 1 - i, 8, bit(bits, i));
  for (let i = 8; i < 15; i += 1) setFunction(grid, 8, grid.size - 15 + i, bit(bits, i));
  setFunction(grid, 8, grid.size - 8, true);
}

function drawVersionBits(grid: Grid): void {
  // Below version 7 the reader infers the version from the symbol's size.
  if (grid.version < 7) return;

  let rem = grid.version;
  for (let i = 0; i < 12; i += 1) rem = (rem << 1) ^ ((rem >>> 11) * 0x1f25);
  const bits = (grid.version << 12) | rem;

  for (let i = 0; i < 18; i += 1) {
    const value = bit(bits, i);
    const a = grid.size - 11 + (i % 3);
    const b = Math.floor(i / 3);
    setFunction(grid, a, b, value);
    setFunction(grid, b, a, value);
  }
}

/** Fill the free modules, walking two columns at a time, upwards then down. */
function drawCodewords(grid: Grid, data: Uint8Array): void {
  let i = 0;
  for (let right = grid.size - 1; right >= 1; right -= 2) {
    // Column 6 is the vertical timing pattern; the pairs step around it.
    if (right === 6) right = 5;

    for (let vertical = 0; vertical < grid.size; vertical += 1) {
      for (let j = 0; j < 2; j += 1) {
        const x = right - j;
        const upward = ((right + 1) & 2) === 0;
        const y = upward ? grid.size - 1 - vertical : vertical;
        if (!grid.isFunction[y][x] && i < data.length * 8) {
          grid.modules[y][x] = bit(data[i >>> 3], 7 - (i & 7));
          i += 1;
        }
      }
    }
  }
}

/* --------------------------------------------------------------------------
 * Masking
 * -------------------------------------------------------------------------- */

function maskInverts(mask: number, x: number, y: number): boolean {
  switch (mask) {
    case 0:
      return (x + y) % 2 === 0;
    case 1:
      return y % 2 === 0;
    case 2:
      return x % 3 === 0;
    case 3:
      return (x + y) % 3 === 0;
    case 4:
      return (Math.floor(x / 3) + Math.floor(y / 2)) % 2 === 0;
    case 5:
      return ((x * y) % 2) + ((x * y) % 3) === 0;
    case 6:
      return (((x * y) % 2) + ((x * y) % 3)) % 2 === 0;
    default:
      return (((x + y) % 2) + ((x * y) % 3)) % 2 === 0;
  }
}

function applyMask(grid: Grid, mask: number): void {
  for (let y = 0; y < grid.size; y += 1) {
    for (let x = 0; x < grid.size; x += 1) {
      if (!grid.isFunction[y][x] && maskInverts(mask, x, y)) {
        grid.modules[y][x] = !grid.modules[y][x];
      }
    }
  }
}

/**
 * The mask with the lowest penalty.
 *
 * All eight are legal and every reader handles all eight; the score is about
 * how easy the symbol is to *find* — large blank fields and accidental
 * finder-like runs are what make a camera hunt.
 */
function chooseMask(grid: Grid): number {
  let best = 0;
  let lowest = Infinity;

  for (let mask = 0; mask < 8; mask += 1) {
    // The format bits carry the mask number, and they are part of what gets
    // scored, so they have to be written before the score is taken.
    drawFormatBits(grid, mask);
    applyMask(grid, mask);
    const penalty = penaltyScore(grid);
    applyMask(grid, mask);

    if (penalty < lowest) {
      lowest = penalty;
      best = mask;
    }
  }

  return best;
}

function penaltyScore(grid: Grid): number {
  const size = grid.size;
  let result = 0;
  let dark = 0;

  for (let y = 0; y < size; y += 1) {
    const row = grid.modules[y];
    const next = y + 1 < size ? grid.modules[y + 1] : null;
    const history = [0, 0, 0, 0, 0, 0, 0];
    let runColour = false;
    let run = 0;

    for (let x = 0; x < size; x += 1) {
      const cell = row[x];
      if (cell) dark += 1;

      if (cell === runColour) {
        run += 1;
        // Rule 1: five in a row costs N1, and every module past that costs one.
        if (run === 5) result += PENALTY_N1;
        else if (run > 5) result += 1;
      } else {
        addRunHistory(history, run, size);
        if (!runColour) result += finderPatterns(history) * PENALTY_N3;
        runColour = cell;
        run = 1;
      }

      // Rule 2: a 2x2 block of one colour, counted from its top-left corner.
      if (next !== null && x + 1 < size && cell === row[x + 1] && cell === next[x] && cell === next[x + 1]) {
        result += PENALTY_N2;
      }
    }
    result += terminateRunHistory(history, runColour, run, size) * PENALTY_N3;
  }

  for (let x = 0; x < size; x += 1) {
    const history = [0, 0, 0, 0, 0, 0, 0];
    let runColour = false;
    let run = 0;

    for (let y = 0; y < size; y += 1) {
      const cell = grid.modules[y][x];
      if (cell === runColour) {
        run += 1;
        if (run === 5) result += PENALTY_N1;
        else if (run > 5) result += 1;
      } else {
        addRunHistory(history, run, size);
        if (!runColour) result += finderPatterns(history) * PENALTY_N3;
        runColour = cell;
        run = 1;
      }
    }
    result += terminateRunHistory(history, runColour, run, size) * PENALTY_N3;
  }

  // Rule 4: how far the balance of dark to light strays from even.
  const total = size * size;
  const k = Math.ceil(Math.abs(dark * 20 - total * 10) / total) - 1;
  return result + k * PENALTY_N4;
}

/** Rule 3: runs in the 1:1:3:1:1 ratio a finder pattern has, either way round. */
function finderPatterns(history: number[]): number {
  const n = history[1];
  const core =
    n > 0 && history[2] === n && history[3] === n * 3 && history[4] === n && history[5] === n;

  let result = 0;
  if (core && history[0] >= n * 4 && history[6] >= n) result += 1;
  if (core && history[6] >= n * 4 && history[0] >= n) result += 1;
  return result;
}

function addRunHistory(history: number[], run: number, size: number): void {
  // The first run of a line is preceded by the quiet zone, which counts as
  // light space when looking for a finder pattern at the edge.
  if (history[0] === 0) run += size;
  history.pop();
  history.unshift(run);
}

function terminateRunHistory(
  history: number[],
  runColour: boolean,
  run: number,
  size: number,
): number {
  if (runColour) {
    addRunHistory(history, run, size);
    run = 0;
  }
  addRunHistory(history, run + size, size);
  return finderPatterns(history);
}

function bit(value: number, index: number): boolean {
  return ((value >>> index) & 1) !== 0;
}

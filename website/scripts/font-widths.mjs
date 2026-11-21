#!/usr/bin/env node
// Prints the advance widths of a WOFF2 font file, per character, in thousandths of an em: printable ASCII, a few
// typographic marks, and the characters given after the file:
//   node scripts/font-widths.mjs public/fonts/ibm-plex-serif-400-normal.woff2 'é'
// A character the font has no glyph for is named on stderr, and the exit code is 1.
// The map's title widths (src/workshop/map-fit.ts) are made with it. It reads the font's cmap and hmtx tables, from
// the file's single Brotli stream; it ignores kerning, which in these titles only narrows a line.
import { readFileSync } from 'node:fs';
import { brotliDecompressSync } from 'node:zlib';

const KNOWN = ['cmap', 'head', 'hhea', 'hmtx', 'maxp', 'name', 'OS/2', 'post', 'cvt ', 'fpgm', 'glyf', 'loca', 'prep', 'CFF ', 'VORG', 'EBDT', 'EBLC', 'gasp', 'hdmx', 'kern', 'LTSH', 'PCLT', 'VDMX', 'vhea', 'vmtx', 'BASE', 'GDEF', 'GPOS', 'GSUB', 'EBSC', 'JSTF', 'MATH', 'CBDT', 'CBLC', 'COLR', 'CPAL', 'SVG ', 'sbix', 'acnt', 'avar', 'bdat', 'bloc', 'bsln', 'cvar', 'fdsc', 'feat', 'fmtx', 'fvar', 'gvar', 'hsty', 'just', 'lcar', 'mort', 'morx', 'opbd', 'prop', 'trak', 'Zapf', 'Silf', 'Glat', 'Gloc', 'Feat', 'Sill'];

/** The tables of a WOFF2 file, decompressed, by tag. */
export function woff2Tables(file) {
  const view = new DataView(file.buffer, file.byteOffset, file.byteLength);
  if (view.getUint32(0) !== 0x774f4632) throw new Error('not a WOFF2 file');
  const count = view.getUint16(12);
  const compressed = view.getUint32(20);
  let at = 48;
  const base128 = () => {
    let value = 0;
    for (let i = 0; i < 5; i += 1) {
      const byte = view.getUint8(at++);
      value = value * 128 + (byte & 0x7f);
      if (!(byte & 0x80)) return value;
    }
    throw new Error('bad UIntBase128');
  };
  const directory = [];
  for (let i = 0; i < count; i += 1) {
    const flags = view.getUint8(at++);
    let tag = KNOWN[flags & 0x3f];
    if ((flags & 0x3f) === 63) { tag = String.fromCharCode(...file.subarray(at, at + 4)); at += 4; }
    const version = flags >> 6;
    const length = base128();
    // glyf and loca are transformed unless their version is 3; any other table is transformed when its version is not 0.
    const transformed = tag === 'glyf' || tag === 'loca' ? version !== 3 : version !== 0;
    directory.push({ tag, length: transformed ? base128() : length, transformed });
  }
  const data = brotliDecompressSync(file.subarray(at, at + compressed));
  const tables = {};
  let offset = 0;
  for (const table of directory) {
    tables[table.tag] = { data: data.subarray(offset, offset + table.length), transformed: table.transformed };
    offset += table.length;
  }
  return tables;
}

/** The glyph of each code point the font maps, from its cmap (format 4 or 12). */
function glyphsOf(cmap) {
  const view = new DataView(cmap.buffer, cmap.byteOffset, cmap.byteLength);
  const subtables = view.getUint16(2);
  let best = null;
  for (let i = 0; i < subtables; i += 1) {
    const platform = view.getUint16(4 + i * 8);
    const encoding = view.getUint16(6 + i * 8);
    const offset = view.getUint32(8 + i * 8);
    const format = view.getUint16(offset);
    if ((platform === 3 && (encoding === 1 || encoding === 10)) || platform === 0) {
      if (format === 12 || (format === 4 && !best)) best = { offset, format };
    }
  }
  if (!best) throw new Error('no Unicode cmap');
  const glyphs = new Map();
  const { offset, format } = best;
  if (format === 4) {
    const segments = view.getUint16(offset + 6) / 2;
    const ends = offset + 14;
    const starts = ends + segments * 2 + 2;
    const deltas = starts + segments * 2;
    const ranges = deltas + segments * 2;
    for (let s = 0; s < segments; s += 1) {
      const end = view.getUint16(ends + s * 2);
      const start = view.getUint16(starts + s * 2);
      const delta = view.getInt16(deltas + s * 2);
      const range = view.getUint16(ranges + s * 2);
      for (let c = start; c <= end && c !== 0xffff; c += 1) {
        let glyph;
        if (!range) glyph = (c + delta) & 0xffff;
        else {
          const at = ranges + s * 2 + range + (c - start) * 2;
          glyph = view.getUint16(at);
          if (glyph) glyph = (glyph + delta) & 0xffff;
        }
        if (glyph) glyphs.set(c, glyph);
      }
    }
  } else {
    const groups = view.getUint32(offset + 12);
    for (let g = 0; g < groups; g += 1) {
      const at = offset + 16 + g * 12;
      const start = view.getUint32(at);
      const end = view.getUint32(at + 4);
      const first = view.getUint32(at + 8);
      for (let c = start; c <= end; c += 1) glyphs.set(c, first + c - start);
    }
  }
  return glyphs;
}

/**
 * The advance width of every character the font maps, in thousandths of an em, rounded.
 * @param {Uint8Array} file @returns {Record<string, number>}
 */
export function advanceWidths(file) {
  const tables = woff2Tables(file);
  const units = new DataView(tables.head.data.buffer, tables.head.data.byteOffset).getUint16(18);
  const metrics = new DataView(tables.hhea.data.buffer, tables.hhea.data.byteOffset).getUint16(34);
  const hmtx = tables.hmtx;
  const view = new DataView(hmtx.data.buffer, hmtx.data.byteOffset, hmtx.data.byteLength);
  // An untransformed hmtx holds (advance, side bearing) pairs; a transformed one starts with a flags byte, then the advances.
  const advance = (glyph) => {
    const index = Math.min(glyph, metrics - 1);
    return hmtx.transformed ? view.getUint16(1 + index * 2) : view.getUint16(index * 4);
  };
  /** @type {Record<string, number>} */
  const widths = {};
  for (const [code, glyph] of glyphsOf(tables.cmap.data)) widths[String.fromCodePoint(code)] = Math.round((advance(glyph) * 1000) / units);
  return widths;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const widths = advanceWidths(readFileSync(process.argv[2]));
  const asked = [...new Set([...(process.argv.slice(3).join(''))])];
  // Printable ASCII and the typographic marks a title may use, then the characters asked for.
  const printable = Object.entries(widths).filter(([char]) => (char >= ' ' && char <= '~') || '‘’“”–—…'.includes(char) || asked.includes(char));
  console.log(JSON.stringify(Object.fromEntries(printable)));
  const missing = asked.filter((char) => !Object.hasOwn(widths, char));
  if (missing.length) {
    console.error(`${process.argv[2]} has no glyph for ${missing.map((char) => `"${char}"`).join(', ')}: the page would draw it in another face.`);
    process.exitCode = 1;
  }
}

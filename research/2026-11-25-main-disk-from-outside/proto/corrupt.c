// What one corrupted bit does to a decmpfs block of LZVN or LZFSE, decoded by Apple's open-source
// lzfse (the decoder #370's reader and #390's probe use off the Mac).
//
//   corrupt FILE... > results
//
// Each file is cut into blocks of 64 KiB (decmpfs's block size), at most BLOCKS of them, spread over
// the files. Each block is encoded twice, as LZFSE (lzfse_encode_buffer) and as LZVN
// (lzvn_encode_buffer, the raw stream decmpfs stores). Then, FLIPS times per block and format, one
// bit is flipped at a random place of the encoded block and the block is decoded by three readers:
//
//   naive   takes what the decoder gives into a buffer of the block's size
//   probe   #390's apfscontent.py: a buffer 64 bytes larger, and the decoder must give exactly the
//           size (LZVN is wrapped in lzfse's own "bvxn" block, as the probe does)
//   strict  the probe, and the whole input consumed: for LZFSE the end-of-stream block "bvx$" is
//           the last 4 bytes; for LZVN the wrapper already requires it
//
// Each outcome is one of: refused (the reader gives an error, a gap), same (the bytes are the
// original's: the bit did not matter), wrong (the reader gives other bytes and no error).
// It prints one line per format and reader, and the share of encoded bytes that are literals,
// which no rule of the format can check.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "lzfse.h"
#include "lzfse_internal.h"

#define BLOCK 65536
#define FLIPS 200

static uint64_t rng = 0x5110;
static uint64_t next(void) {
  rng ^= rng << 13; rng ^= rng >> 7; rng ^= rng << 17; return rng;
}

enum { REFUSED, SAME, WRONG };
static long counts[2][3][3];  // [lzfse, lzvn][naive, probe, strict][outcome]
static long blocks[2], in_bytes[2], out_bytes[2];

static uint8_t dst[BLOCK + 64], enc[BLOCK * 2], work[BLOCK * 2 + 32];
static void *scratch;

// lzfse's buffer API, plus where it stopped reading.
static size_t decode(uint8_t *d, size_t cap, const uint8_t *s, size_t n, size_t *used) {
  lzfse_decoder_state *st = scratch;
  memset(st, 0, sizeof(*st));
  st->src = s; st->src_begin = s; st->src_end = s + n;
  st->dst = d; st->dst_begin = d; st->dst_end = d + cap;
  int status = lzfse_decode(st);
  *used = (size_t)(st->src - s);
  if (status == LZFSE_STATUS_DST_FULL) return cap;
  if (status != LZFSE_STATUS_OK) return 0;
  return (size_t)(st->dst - d);
}

static int outcome(const uint8_t *orig, size_t size, const uint8_t *got, size_t n, int ok) {
  if (!ok) return REFUSED;
  if (n == size && memcmp(orig, got, size) == 0) return SAME;
  return WRONG;
}

// The three readers on one encoded block.
static void readers(int fmt, const uint8_t *orig, size_t size, const uint8_t *e, size_t elen, int out[3]) {
  const uint8_t *s = e; size_t n = elen, used;
  if (fmt == 1) {  // LZVN: wrap as the probe does
    memcpy(work, "bvxn", 4);
    uint32_t a = (uint32_t)size, b = (uint32_t)elen;
    memcpy(work + 4, &a, 4); memcpy(work + 8, &b, 4);
    memcpy(work + 12, e, elen); memcpy(work + 12 + elen, "bvx$", 4);
    s = work; n = elen + 16;
  }
  size_t got = decode(dst, size, s, n, &used);
  out[0] = outcome(orig, size, dst, got, got > 0 || size == 0);
  got = decode(dst, size + 64, s, n, &used);
  int probe_ok = got == size;
  out[1] = outcome(orig, size, dst, got, probe_ok);
  out[2] = outcome(orig, size, dst, got, probe_ok && used == n);
}

static int literal_bytes(const uint8_t *e, size_t n);

int main(int argc, char **argv) {
  scratch = malloc(lzfse_decode_scratch_size() + lzfse_encode_scratch_size() + lzvn_encode_scratch_size());
  void *escratch = malloc(lzfse_encode_scratch_size() + lzvn_encode_scratch_size() + 1);
  int max_blocks = getenv("BLOCKS") ? atoi(getenv("BLOCKS")) : 400;
  int per_file = max_blocks / (argc - 1) + 1;
  static uint8_t buf[BLOCK];
  long lits = 0, lzvn_total = 0;
  for (int f = 1; f < argc; f++) {
    FILE *fp = fopen(argv[f], "rb");
    if (!fp) continue;
    for (int k = 0; k < per_file; k++) {
      size_t size = fread(buf, 1, BLOCK, fp);
      if (size < 4096) break;
      for (int fmt = 0; fmt < 2; fmt++) {
        size_t elen = fmt == 0 ? lzfse_encode_buffer(enc, sizeof enc, buf, size, escratch)
                               : lzvn_encode_buffer(enc, sizeof enc, buf, size, escratch);
        if (elen == 0 || elen >= size) continue;  // decmpfs stores such a block uncompressed
        int out[3];
        readers(fmt, buf, size, enc, elen, out);
        if (out[2] != SAME) { fprintf(stderr, "clean block not read back (fmt %d)\n", fmt); return 1; }
        blocks[fmt]++; in_bytes[fmt] += size; out_bytes[fmt] += elen;
        if (fmt == 1) { lits += literal_bytes(enc, elen); lzvn_total += elen; }
        for (int i = 0; i < FLIPS; i++) {
          size_t at = next() % elen;
          uint8_t bit = (uint8_t)(1u << (next() % 8));
          enc[at] ^= bit;
          readers(fmt, buf, size, enc, elen, out);
          for (int r = 0; r < 3; r++) counts[fmt][r][out[r]]++;
          enc[at] ^= bit;
        }
      }
    }
    fclose(fp);
  }
  const char *fmts[] = {"lzfse", "lzvn"}, *rs[] = {"naive", "probe", "strict"};
  for (int fmt = 0; fmt < 2; fmt++) {
    printf("%s: %ld blocks, %ld bytes encoded to %ld (%.1f%%), %d flips each\n", fmts[fmt], blocks[fmt],
           in_bytes[fmt], out_bytes[fmt], 100.0 * out_bytes[fmt] / (in_bytes[fmt] ? in_bytes[fmt] : 1), FLIPS);
    for (int r = 0; r < 3; r++) {
      long t = counts[fmt][r][0] + counts[fmt][r][1] + counts[fmt][r][2];
      printf("  %-6s refused %5.1f%%  same %5.1f%%  wrong without an error %5.1f%%\n", rs[r],
             100.0 * counts[fmt][r][REFUSED] / t, 100.0 * counts[fmt][r][SAME] / t,
             100.0 * counts[fmt][r][WRONG] / t);
    }
  }
  printf("lzvn: %.1f%% of encoded bytes are literals, copied to the output as they are\n",
         100.0 * lits / (lzvn_total ? lzvn_total : 1));
  return 0;
}

// Counts the literal bytes of an LZVN stream, by its opcodes (lzvn_decode_base.c's table).
static int literal_bytes(const uint8_t *e, size_t n) {
  size_t i = 0; int lit = 0;
  while (i < n) {
    uint8_t op = e[i];
    size_t L = 0, len = 1;
    if (op == 0x06) break;                                   // end of stream
    if ((op & 0xF0) == 0xE0) { L = op & 0x0F; len = 1; }     // small literal
    else if (op == 0xE0) { L = e[i + 1] + 16; len = 2; }     // large literal
    else if ((op & 0xF0) == 0xF0) { len = op == 0xF0 ? 2 : 1; }  // match only
    else if ((op & 0xF0) == 0x70 || (op & 0xF0) == 0xD0) { len = 1; }  // undefined
    else if ((op & 0xE0) == 0xA0) { L = (op >> 3) & 3; len = 3; }  // medium distance
    else if ((op & 0x07) == 0x07) { L = (op >> 6) & 3; len = 3; }  // large distance
    else if ((op & 0x07) == 0x06) { L = (op >> 6) & 3; len = 1; }  // previous distance
    else { L = (op >> 6) & 3; len = 2; }                     // small distance
    if (op == 0xE0) { L = e[i + 1] + 16; len = 2; }
    lit += (int)L;
    i += len + L;
  }
  return lit;
}

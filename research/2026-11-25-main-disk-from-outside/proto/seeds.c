// Writes seed inputs for fuzz_lzfse.py: each file of the arguments, cut in pieces of SIZE bytes
// (default 4096), encoded as LZFSE and as LZVN, in the fuzzer's input format (one byte of format,
// two of size minus one, then the encoded block).
//
//   seeds OUTDIR FILE...
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "lzfse.h"
#include "lzfse_internal.h"

int main(int argc, char **argv) {
  size_t piece = getenv("SIZE") ? (size_t)atoi(getenv("SIZE")) : 4096;
  static uint8_t in[65536], out[3 + 2 * 65536];
  void *scratch = malloc(lzfse_encode_scratch_size() + lzvn_encode_scratch_size() + 1);
  int n = 0;
  for (int f = 2; f < argc; f++) {
    FILE *fp = fopen(argv[f], "rb");
    if (!fp) continue;
    for (int k = 0; k < 4; k++) {
      size_t size = fread(in, 1, piece, fp);
      if (size == 0) break;
      for (int fmt = 0; fmt < 2; fmt++) {
        size_t e = fmt == 0 ? lzfse_encode_buffer(out + 3, sizeof out - 3, in, size, scratch)
                            : lzvn_encode_buffer(out + 3, sizeof out - 3, in, size, scratch);
        if (e == 0) continue;
        out[0] = (uint8_t)fmt;
        uint16_t s = (uint16_t)(size - 1);
        memcpy(out + 1, &s, 2);
        char name[4096];
        snprintf(name, sizeof name, "%s/seed-%03d", argv[1], n++);
        FILE *o = fopen(name, "wb");
        fwrite(out, 1, e + 3, o);
        fclose(o);
      }
    }
    fclose(fp);
  }
  printf("%d seeds\n", n);
  return 0;
}

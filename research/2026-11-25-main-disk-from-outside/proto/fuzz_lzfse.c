// A small coverage-guided fuzzer for Apple's open-source lzfse decoder, on input root in main
// controls: a decmpfs block of LZFSE, or of LZVN wrapped in a "bvxn" block as #390's probe does.
//
// The decoder is built with gcc's AddressSanitizer and UndefinedBehaviorSanitizer and with
// -fsanitize-coverage=trace-pc: each basic block it runs calls __sanitizer_cov_trace_pc below,
// which marks its address in a map. An input that reaches a block never reached before joins the
// corpus. Mutations: bit flips, bytes set to edge values, runs copied, bytes inserted and removed,
// pieces of two inputs joined, and the size the reader asks for changed.
//
//   fuzz_lzfse SEEDS_DIR SECONDS        (see run-fuzz.sh)
//
// Input format, as seeds.c writes it: one byte of format (0 LZFSE, 1 LZVN), two of block size
// minus one, then the encoded block. Every input is decoded into a buffer of the size plus 64,
// as the probe allocates, from a heap copy of exactly its length, so any read past the input or
// write past the buffer stops the run with the sanitizer's report.
#include <dirent.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <fcntl.h>
#include <sys/mman.h>
#include <unistd.h>

#include "lzfse.h"

#define MAP (1 << 20)
static uint8_t map[MAP];
static int fresh;

// Called by the instrumented decoder at each basic block. Not instrumented itself.
__attribute__((no_sanitize_address, no_instrument_function)) void __sanitizer_cov_trace_pc(void) {
  uintptr_t pc = (uintptr_t)__builtin_return_address(0);
  uint32_t i = (uint32_t)((pc ^ (pc >> 20)) & (MAP - 1));
  if (!map[i]) { map[i] = 1; fresh++; }
}

static uint64_t rng = 0x5110;
static uint64_t next(void) { rng ^= rng << 13; rng ^= rng >> 7; rng ^= rng << 17; return rng; }

typedef struct { uint8_t *d; size_t n; } input;
static input corpus[20000];
static int ncorpus;
static void *scratch;
static size_t pad;

// The input being run is kept in the file crash-input (mapped: 8 bytes of length, then the
// input), so that it is there when a sanitizer stops the run.
static uint8_t *current;

static void run(const uint8_t *data, size_t n) {
  if (current) { uint64_t len = n; memcpy(current, &len, 8); memcpy(current + 8, data, n); }
  if (n < 3) return;
  int fmt = data[0] & 1;
  size_t size = (size_t)(data[1] | data[2] << 8) + 1;
  size_t blen = n - 3 + (fmt ? 16 : 0);
  // PAD bytes of zeroes after the input, which the decoder is not told about: PAD=8 is the rule
  // the over-read found by this fuzzer calls for.
  uint8_t *src = calloc(1, blen + pad ? blen + pad : 1);
  if (fmt) {
    uint32_t a = (uint32_t)size, b = (uint32_t)(n - 3);
    memcpy(src, "bvxn", 4); memcpy(src + 4, &a, 4); memcpy(src + 8, &b, 4);
    memcpy(src + 12, data + 3, n - 3); memcpy(src + 12 + n - 3, "bvx$", 4);
  } else {
    memcpy(src, data + 3, n - 3);
  }
  uint8_t *dst = malloc(size + 64);
  lzfse_decode_buffer(dst, size + 64, src, blen, scratch);
  free(dst);
  free(src);
}

static const uint8_t edges[] = {0, 1, 0x7f, 0x80, 0xff, 0xfe, 0x06, 0xe0, 0xf0};

static size_t mutate(uint8_t *d, size_t n, size_t cap) {
  int rounds = 1 + (int)(next() % 4);
  for (int r = 0; r < rounds; r++) {
    if (n < 4) return n;
    size_t at = 3 + next() % (n - 3);
    switch (next() % 9) {
      case 0: d[at] ^= (uint8_t)(1u << (next() % 8)); break;
      case 1: d[at] = edges[next() % sizeof edges]; break;
      case 2: d[at] = (uint8_t)next(); break;
      case 3: { uint32_t v = (uint32_t)next() % 70000; if (at + 4 <= n) memcpy(d + at, &v, 4); } break;
      case 4: { size_t len = 1 + next() % 16, from = 3 + next() % (n - 3);
                if (at + len <= n && from + len <= n) memmove(d + at, d + from, len); } break;
      case 5: if (n + 1 < cap) { memmove(d + at + 1, d + at, n - at); d[at] = (uint8_t)next(); n++; } break;
      case 6: memmove(d + at, d + at + 1, n - at - 1); n--; break;
      case 7: { const input *o = &corpus[next() % ncorpus]; size_t from = 3 + next() % (o->n > 3 ? o->n - 3 : 1);
                size_t len = o->n > from ? o->n - from : 0; if (at + len > cap) len = cap - at;
                memcpy(d + at, o->d + from, len); n = at + len; } break;
      case 8: { uint16_t s = (uint16_t)next(); memcpy(d + 1, &s, 2); } break;
    }
  }
  return n;
}

static void keep(const uint8_t *d, size_t n) {
  if (ncorpus >= (int)(sizeof corpus / sizeof corpus[0])) return;
  corpus[ncorpus].d = malloc(n); memcpy(corpus[ncorpus].d, d, n); corpus[ncorpus].n = n; ncorpus++;
}

int main(int argc, char **argv) {
  scratch = malloc(lzfse_decode_scratch_size() + 1);
  pad = getenv("PAD") ? (size_t)atoi(getenv("PAD")) : 0;
  if (argc == 2) {  // replay one input, as crash-input keeps it
    static uint8_t one[(1 << 17) + 8];
    FILE *f = fopen(argv[1], "rb");
    size_t n = fread(one, 1, sizeof one, f);
    fclose(f);
    uint64_t len; memcpy(&len, one, 8);
    if (len > n - 8) len = n - 8;
    run(one + 8, len);
    printf("no fault\n");
    return 0;
  }
  int fd = open("crash-input", O_RDWR | O_CREAT | O_TRUNC, 0644);
  if (fd >= 0 && ftruncate(fd, (1 << 17) + 8) == 0)
    current = mmap(NULL, (1 << 17) + 8, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
  if (current == MAP_FAILED) current = NULL;
  DIR *dir = opendir(argv[1]);
  struct dirent *e;
  static uint8_t buf[1 << 17];
  while (dir && (e = readdir(dir))) {
    if (e->d_name[0] == '.') continue;
    char p[4096]; snprintf(p, sizeof p, "%s/%s", argv[1], e->d_name);
    FILE *f = fopen(p, "rb"); if (!f) continue;
    size_t n = fread(buf, 1, sizeof buf, f); fclose(f);
    fresh = 0; run(buf, n); keep(buf, n);
  }
  int seeds = ncorpus;
  time_t end = time(NULL) + atol(argv[2]);
  long execs = 0;
  while (time(NULL) < end) {
    for (int k = 0; k < 1000; k++) {
      const input *base = &corpus[next() % ncorpus];
      size_t n = base->n < sizeof buf ? base->n : sizeof buf;
      memcpy(buf, base->d, n);
      n = mutate(buf, n, sizeof buf);
      fresh = 0;
      run(buf, n);
      execs++;
      if (fresh) keep(buf, n);
    }
  }
  int edges_hit = 0;
  for (int i = 0; i < MAP; i++) edges_hit += map[i];
  printf("%ld inputs run, %d seeds, corpus %d, %d decoder blocks reached, no fault\n", execs, seeds,
         ncorpus, edges_hit);
  return 0;
}

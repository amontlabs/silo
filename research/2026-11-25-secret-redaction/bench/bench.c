// The cost of finding known values in what the recorder writes, in MB/s.
//
//   bench <corpus> <patterns> pairs|anchors
//
// `pairs` is #386's redactor: a bit per pair of starting bytes, then each pattern compared in turn.
// `anchors` looks up every window of 16 bytes of every pattern, taken every 8 bytes, through a bit
// table on a hash of the first 8 bytes. It only counts the places that match; extending a match
// across line breaks runs only there, so it does not change the cost of the scan.
#define _GNU_SOURCE
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static char *slurp(const char *p, size_t *n) {
    FILE *f = fopen(p, "rb");
    if (!f) { perror(p); exit(1); }
    fseek(f, 0, SEEK_END); *n = ftell(f); fseek(f, 0, SEEK_SET);
    char *b = malloc(*n + 16);
    if (fread(b, 1, *n, f) != *n) exit(1);
    memset(b + *n, 0, 16);
    fclose(f);
    return b;
}

static double now(void) {
    struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec + t.tv_nsec / 1e9;
}

typedef struct { const char *s; size_t n; } pat;

static size_t run_pairs(const char *in, size_t n, pat *ps, size_t np) {
    static uint8_t starts[65536 / 8];
    memset(starts, 0, sizeof starts);
    for (size_t k = 0; k < np; k++) {
        uint16_t key = (uint8_t)ps[k].s[0] << 8 | (uint8_t)ps[k].s[1];
        starts[key >> 3] |= 1 << (key & 7);
    }
    size_t count = 0, i = 0;
    while (i < n) {
        uint16_t key = i + 1 < n ? (uint8_t)in[i] << 8 | (uint8_t)in[i + 1] : 0;
        if (i + 1 >= n || !(starts[key >> 3] & (1 << (key & 7)))) { i++; continue; }
        size_t best = 0;
        for (size_t k = 0; k < np; k++)
            if (n - i >= ps[k].n && in[i] == ps[k].s[0] && memcmp(in + i, ps[k].s, ps[k].n) == 0 && ps[k].n > best)
                best = ps[k].n;
        if (best) { count++; i += best; } else i++;
    }
    return count;
}

#define WIN 16
#define STRIDE 8
#define BITS 22

static inline uint64_t h64(uint64_t x) { x ^= x >> 33; x *= 0xff51afd7ed558ccdULL; x ^= x >> 33; return x; }

typedef struct { uint64_t a, b; } win;
static int cmpwin(const void *x, const void *y) {
    const win *p = x, *q = y;
    return p->a < q->a ? -1 : p->a > q->a ? 1 : p->b < q->b ? -1 : p->b > q->b;
}

static size_t run_anchors(const char *in, size_t n, pat *ps, size_t np, size_t *nwin) {
    uint64_t *bloom = calloc((1u << BITS) / 64, 8);
    size_t cap = 0;
    for (size_t k = 0; k < np; k++) cap += ps[k].n / STRIDE + 2;
    if (!cap) cap = 1;
    win *w = malloc(cap * sizeof *w);
    size_t nw = 0;
    for (size_t k = 0; k < np; k++) {
        if (ps[k].n < WIN) continue;
        for (size_t off = 0;; off += STRIDE) {
            if (off + WIN > ps[k].n) off = ps[k].n - WIN;
            memcpy(&w[nw].a, ps[k].s + off, 8); memcpy(&w[nw].b, ps[k].s + off + 8, 8);
            uint64_t h = h64(w[nw].a) >> (64 - BITS);
            bloom[h >> 6] |= 1ULL << (h & 63);
            nw++;
            if (off + WIN >= ps[k].n) break;
        }
    }
    qsort(w, nw, sizeof *w, cmpwin);
    *nwin = nw;
    size_t count = 0;
    for (size_t i = 0; i + WIN <= n; i++) {
        uint64_t a; memcpy(&a, in + i, 8);
        uint64_t h = h64(a) >> (64 - BITS);
        if (!(bloom[h >> 6] & (1ULL << (h & 63)))) continue;
        win key; key.a = a; memcpy(&key.b, in + i + 8, 8);
        if (bsearch(&key, w, nw, sizeof *w, cmpwin)) { count++; i += WIN - 1; }
    }
    free(bloom); free(w);
    return count;
}

int main(int argc, char **argv) {
    if (argc != 4) { fprintf(stderr, "usage: bench <corpus> <patterns> pairs|anchors\n"); return 2; }
    size_t n, pn;
    char *in = slurp(argv[1], &n), *praw = slurp(argv[2], &pn);
    // Patterns: one per line, with \n inside a pattern written as \x01.
    size_t np = 0; pat *ps = malloc(sizeof(pat) * (pn / 8 + 1));
    for (char *p = praw, *e; p < praw + pn; p = e + 1) {
        e = memchr(p, '\n', praw + pn - p); if (!e) e = praw + pn;
        for (char *c = p; c < e; c++) if (*c == 1) *c = '\n';
        if (e - p >= 8) { ps[np].s = p; ps[np].n = e - p; np++; }
    }
    double best = 1e9; size_t hits = 0, nwin = 0;
    for (int r = 0; r < 5; r++) {
        double t = now();
        hits = strcmp(argv[3], "pairs") == 0 ? run_pairs(in, n, ps, np) : run_anchors(in, n, ps, np, &nwin);
        t = now() - t;
        if (t < best) best = t;
    }
    printf("{\"method\":\"%s\",\"patterns\":%zu,\"windows\":%zu,\"bytes\":%zu,\"hits\":%zu,\"mb_per_s\":%.1f}\n",
           argv[3], np, nwin, n, hits, n / best / 1e6);
    return 0;
}

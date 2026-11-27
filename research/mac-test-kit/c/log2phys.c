/* log2phys: which blocks of two clones of one file differ, from the device map alone, checked
 * against a byte comparison.
 *
 *   log2phys diff A B [--bytes] [--block N]
 *
 * Prints one JSON object. From the maps (F_LOG2PHYS_EXT on a Mac, FIEMAP on Linux):
 *   map_seconds, pieces_a, pieces_b, differ_ranges, differ_blocks (blocks whose device offset
 *   differs, or that are data in one file and a hole in the other).
 * With --bytes, both files are read in full and each block is classed:
 *   bytes_differ        blocks whose bytes differ
 *   missed              bytes differ but the map says same place: the map cannot be trusted
 *   extra               the map says differ but the bytes are equal (a rewrite of equal bytes,
 *                       or a block moved): harmless, it costs a read
 *   compare_seconds
 * The block is 4096 bytes unless --block says otherwise.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>
#ifdef __linux__
#include <linux/fiemap.h>
#include <linux/fs.h>
#endif

typedef struct { long long lo, dev, len; } piece_t;
typedef struct { piece_t *p; size_t n, cap; } map_t;

static double now(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec + t.tv_nsec / 1e9;
}

static void add(map_t *m, long long lo, long long dev, long long len) {
  if (m->n && m->p[m->n - 1].lo + m->p[m->n - 1].len == lo && m->p[m->n - 1].dev + m->p[m->n - 1].len == dev) {
    m->p[m->n - 1].len += len;
    return;
  }
  if (m->n == m->cap) {
    m->cap = m->cap ? m->cap * 2 : 1024;
    m->p = realloc(m->p, m->cap * sizeof(piece_t));
    if (!m->p) { perror("realloc"); exit(1); }
  }
  m->p[m->n].lo = lo; m->p[m->n].dev = dev; m->p[m->n].len = len;
  m->n++;
}

#ifdef __APPLE__
static int getmap(int fd, long long size, map_t *m) {
  long long off = 0;
  while (off < size) {
    off_t d = lseek(fd, off, SEEK_DATA);
    if (d < 0) {
      if (errno == ENXIO) break;
      if (errno == EINVAL || errno == ENOTSUP) d = off;
      else { perror("lseek SEEK_DATA"); return 1; }
    }
    off_t h = lseek(fd, d, SEEK_HOLE);
    if (h < 0) h = size;
    long long p = d;
    while (p < h) {
      struct log2phys l;
      memset(&l, 0, sizeof l);
      l.l2p_contigbytes = h - p;
      l.l2p_devoffset = p;
      if (fcntl(fd, F_LOG2PHYS_EXT, &l) < 0 || l.l2p_contigbytes <= 0) { p += 4096; continue; }
      long long len = l.l2p_contigbytes;
      if (len > h - p) len = h - p;
      if (l.l2p_devoffset >= 0) add(m, p, l.l2p_devoffset, len); /* < 0: not allocated yet */
      p += len;
    }
    off = h;
  }
  return 0;
}
#elif defined(__linux__)
static int getmap(int fd, long long size, map_t *m) {
  enum { N = 4096 };
  size_t sz = sizeof(struct fiemap) + N * sizeof(struct fiemap_extent);
  struct fiemap *fm = calloc(1, sz);
  unsigned long long start = 0;
  (void)size;
  for (;;) {
    memset(fm, 0, sz);
    fm->fm_start = start;
    fm->fm_length = ~0ULL - start;
    fm->fm_flags = FIEMAP_FLAG_SYNC;
    fm->fm_extent_count = N;
    if (ioctl(fd, FS_IOC_FIEMAP, fm) < 0) { perror("FIEMAP"); free(fm); return 1; }
    if (fm->fm_mapped_extents == 0) break;
    int last = 0;
    for (unsigned i = 0; i < fm->fm_mapped_extents; i++) {
      struct fiemap_extent *e = &fm->fm_extents[i];
      add(m, (long long)e->fe_logical, (long long)e->fe_physical, (long long)e->fe_length);
      if (e->fe_flags & FIEMAP_EXTENT_LAST) last = 1;
      start = e->fe_logical + e->fe_length;
    }
    if (last) break;
  }
  free(fm);
  return 0;
}
#else
static int getmap(int fd, long long size, map_t *m) {
  (void)fd; (void)size; (void)m;
  fprintf(stderr, "log2phys: not on this system (needs F_LOG2PHYS_EXT or FIEMAP)\n");
  return 3;
}
#endif

/* device offset of file offset x in m, or -1 for a hole; *run gets how far that answer holds */
static long long where(const map_t *m, size_t *i, long long x, long long *run) {
  while (*i < m->n && m->p[*i].lo + m->p[*i].len <= x) (*i)++;
  if (*i < m->n && m->p[*i].lo <= x) {
    *run = m->p[*i].lo + m->p[*i].len - x;
    return m->p[*i].dev + (x - m->p[*i].lo);
  }
  *run = (*i < m->n) ? m->p[*i].lo - x : (1LL << 62);
  return -1;
}

int main(int argc, char **argv) {
  const char *a = NULL, *b = NULL;
  int bytes = 0;
  long long bs = 4096;
  if (argc < 4 || strcmp(argv[1], "diff")) {
    fprintf(stderr, "usage: log2phys diff A B [--bytes] [--block N]\n");
    return 2;
  }
  for (int i = 2; i < argc; i++) {
    if (!strcmp(argv[i], "--bytes")) bytes = 1;
    else if (!strcmp(argv[i], "--block") && i + 1 < argc) bs = atoll(argv[++i]);
    else if (!a) a = argv[i];
    else b = argv[i];
  }
  if (!a || !b || bs <= 0) return 2;
  int fa = open(a, O_RDONLY), fb = open(b, O_RDONLY);
  if (fa < 0 || fb < 0) { perror("open"); return 1; }
  struct stat sa, sb;
  fstat(fa, &sa); fstat(fb, &sb);
  long long size = sa.st_size > sb.st_size ? sa.st_size : sb.st_size;
  map_t ma = {0}, mb = {0};
#ifdef __APPLE__
  fcntl(fa, F_FULLFSYNC); /* delayed allocations get their place first */
  fcntl(fb, F_FULLFSYNC);
#endif
  double t0 = now();
  int rc = getmap(fa, sa.st_size, &ma);
  if (!rc) rc = getmap(fb, sb.st_size, &mb);
  double t1 = now();
  if (rc) return rc;

  /* sweep by runs where both answers hold */
  size_t ia = 0, ib = 0;
  long long x = 0, differ_blocks = 0, ranges = 0, in_range = 0;
  /* per-block flag kept only when --bytes asks for it: one bit per block */
  long long nblk = (size + bs - 1) / bs;
  unsigned char *flag = bytes ? calloc((size_t)(nblk / 8 + 1), 1) : NULL;
  while (x < size) {
    long long ra, rb;
    long long da = where(&ma, &ia, x, &ra), db = where(&mb, &ib, x, &rb);
    long long run = ra < rb ? ra : rb;
    if (run <= 0) run = bs;
    if (x + run > size) run = size - x;
    int differ = !(da == db); /* both holes: -1 == -1, same */
    if (differ) {
      long long b0 = x / bs, b1 = (x + run + bs - 1) / bs;
      differ_blocks += b1 - b0;
      if (flag) for (long long k = b0; k < b1; k++) flag[k >> 3] |= (unsigned char)(1 << (k & 7));
      if (!in_range) ranges++;
    }
    in_range = differ;
    x += run;
  }
  printf("{\"size\":%lld,\"block\":%lld,\"map_seconds\":%.4f,\"pieces_a\":%zu,\"pieces_b\":%zu,"
         "\"differ_ranges\":%lld,\"differ_blocks\":%lld",
         size, bs, t1 - t0, ma.n, mb.n, ranges, differ_blocks);
  if (bytes) {
    size_t chunk = 1 << 20;
    if ((long long)chunk % bs) chunk = (size_t)bs * 256;
    char *ba = malloc(chunk), *bb = malloc(chunk);
#ifdef __APPLE__
    fcntl(fa, F_NOCACHE, 1);
    fcntl(fb, F_NOCACHE, 1);
#endif
    long long bytes_differ = 0, missed = 0, extra = 0;
    double t2 = now();
    for (long long off = 0; off < size; off += (long long)chunk) {
      ssize_t na = pread(fa, ba, chunk, off), nb = pread(fb, bb, chunk, off);
      if (na < 0 || nb < 0) { perror("pread"); return 1; }
      if ((size_t)na < chunk) memset(ba + na, 0, chunk - (size_t)na);
      if ((size_t)nb < chunk) memset(bb + nb, 0, chunk - (size_t)nb);
      for (long long o = 0; o < (long long)chunk && off + o < size; o += bs) {
        long long k = (off + o) / bs;
        int d = memcmp(ba + o, bb + o, (size_t)bs) != 0;
        int f = (flag[k >> 3] >> (k & 7)) & 1;
        bytes_differ += d;
        if (d && !f) missed++;
        if (!d && f) extra++;
      }
    }
    printf(",\"bytes_differ\":%lld,\"missed\":%lld,\"extra\":%lld,\"compare_seconds\":%.3f",
           bytes_differ, missed, extra, now() - t2);
  }
  printf("}\n");
  return 0;
}

/* fsyncbench: what one forced write costs.
 *
 *   fsyncbench DIR MODE RECORD_BYTES RECORDS_PER_FLUSH FLUSHES PREALLOCATED
 *
 * MODE is none, fsync, barrier (F_BARRIERFSYNC) or full (F_FULLFSYNC). It writes
 * RECORDS_PER_FLUSH records with one write, then flushes, FLUSHES times, into a new file in DIR
 * (preallocated and flushed first when PREALLOCATED is 1, else appended), then deletes it.
 * Prints one CSV line: mode,record_bytes,records_per_flush,preallocated,flushes,p50_us,p99_us,
 * max_us,records_per_s,flush_call. flush_call says what the mode really called: on Linux,
 * barrier and full fall back to fdatasync, which has no barrier-only form.
 * Ported from the benchmark of research 02 (macbench/silo-fsync-bench.sh).
 */
#define _GNU_SOURCE
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#ifdef __APPLE__
#ifndef F_BARRIERFSYNC
#define F_BARRIERFSYNC 85
#endif
#endif

static double us(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec * 1e6 + t.tv_nsec / 1e3;
}
static int cmp(const void *a, const void *b) {
  double x = *(const double *)a, y = *(const double *)b;
  return x < y ? -1 : x > y;
}
static const char *call_name(const char *m) {
  if (!strcmp(m, "none")) return "none";
  if (!strcmp(m, "fsync")) return "fsync";
#ifdef __APPLE__
  if (!strcmp(m, "barrier")) return "F_BARRIERFSYNC";
  if (!strcmp(m, "full")) return "F_FULLFSYNC";
#else
  if (!strcmp(m, "barrier") || !strcmp(m, "full")) return "fdatasync";
#endif
  return "?";
}
static int flush(int fd, const char *m) {
  if (!strcmp(m, "none")) return 0;
  if (!strcmp(m, "fsync")) return fsync(fd);
#ifdef __APPLE__
  if (!strcmp(m, "barrier")) return fcntl(fd, F_BARRIERFSYNC);
  if (!strcmp(m, "full")) return fcntl(fd, F_FULLFSYNC);
#else
  if (!strcmp(m, "barrier") || !strcmp(m, "full")) return fdatasync(fd);
#endif
  return -1;
}
int main(int c, char **v) {
  if (c < 7) {
    fprintf(stderr, "usage: fsyncbench DIR MODE RECORD_BYTES RECORDS_PER_FLUSH FLUSHES PREALLOCATED\n");
    return 2;
  }
  const char *m = v[2];
  int rs = atoi(v[3]), bt = atoi(v[4]), n = atoi(v[5]), pre = atoi(v[6]);
  if (rs <= 0 || bt <= 0 || n <= 0 || !strcmp(call_name(m), "?")) return 2;
  char p[4096];
  snprintf(p, sizeof p, "%s/log.%d", v[1], (int)getpid());
  int fd = open(p, O_RDWR | O_CREAT | O_TRUNC, 0600);
  if (fd < 0) { perror("open"); return 1; }
  long tot = (long)rs * bt * n;
  size_t chunk = 1 << 20;
  if (pre) {
    char *z = calloc(1, chunk);
    for (long o = 0; o < tot; o += (long)chunk) {
      size_t k = (size_t)((tot - o) < (long)chunk ? (tot - o) : (long)chunk);
      if (write(fd, z, k) != (ssize_t)k) { perror("prealloc"); return 1; }
    }
    if (flush(fd, "full")) { perror("flush"); return 1; }
    lseek(fd, 0, SEEK_SET);
    free(z);
  }
  size_t len = (size_t)rs * bt;
  char *b = malloc(len);
  memset(b, 'x', len);
  double *l = malloc(sizeof(double) * n);
  double t0 = us();
  for (int i = 0; i < n; i++) {
    double a = us();
    if (write(fd, b, len) != (ssize_t)len) { perror("write"); return 1; }
    if (flush(fd, m)) { perror(m); return 1; }
    l[i] = us() - a;
  }
  double el = us() - t0;
  qsort(l, n, sizeof(double), cmp);
  printf("%s,%d,%d,%d,%d,%.0f,%.0f,%.0f,%.0f,%s\n", m, rs, bt, pre, n, l[n / 2], l[(int)(n * 0.99)], l[n - 1],
         (double)n * bt / (el / 1e6), call_name(m));
  close(fd);
  unlink(p);
  return 0;
}

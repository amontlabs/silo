/* clonebench: what cloning a disk image costs, and what a program writing to it waits meanwhile.
 *
 *   clonebench fill FILE MIB                     write MIB MiB of random bytes, then flush
 *   clonebench frag FILE N SEED [MINKB MAXKB] [--size BYTES]
 *                                                N writes of MINKB..MAXKB (4..64) KiB at random
 *                                                4 KiB-aligned places, then flush. --size for a
 *                                                device, whose size fstat does not give
 *   clonebench clone SRC DST                     clonefile(2) (FICLONE on Linux), timed
 *   clonebench stall --write PATH [--size BYTES] [--region BYTES] [--cached] [--pre S] [--post S]
 *                    (--clone SRC DST | --delete PATH... | --none)
 *                                                a writer thread writes 4 KiB blocks one after
 *                                                another at random places of the first REGION
 *                                                bytes of PATH (16 MiB; 0 for all of it),
 *                                                uncached unless --cached, while the action runs
 *
 * Every command prints one JSON object. Times are seconds. In stall: action_s is the action's
 * own time; longest_during_s the longest single write that overlapped the action;
 * writes_during how many writes finished inside it; base_p50_s and base_p99_s the writes
 * before the action, the writer's usual speed.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>
#ifdef __APPLE__
#include <sys/clonefile.h>
#include <sys/disk.h>
#endif
#ifdef __linux__
#include <linux/fs.h>
#endif

static double now(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec + t.tv_nsec / 1e9;
}

static uint64_t rng = 88172645463325252ULL;
static uint64_t xr(void) { rng ^= rng << 13; rng ^= rng >> 7; rng ^= rng << 17; return rng; }

static void *abuf(size_t n) {
  void *p = NULL;
  if (posix_memalign(&p, 4096, n)) { perror("posix_memalign"); exit(1); }
  for (size_t i = 0; i < n; i += 8) { uint64_t v = xr(); memcpy((char *)p + i, &v, 8); }
  return p;
}

/* open for writes that skip the page cache, as a VM's uncached disk does */
static int open_w(const char *path, int flags, int cached) {
#ifdef __linux__
  if (!cached) flags |= O_DIRECT;
#endif
  int fd = open(path, flags, 0644);
  if (fd < 0) { perror(path); exit(1); }
#ifdef __APPLE__
  if (!cached) fcntl(fd, F_NOCACHE, 1);
#endif
  return fd;
}

static long long dev_size(int fd) {
  struct stat st;
  if (fstat(fd, &st) == 0 && st.st_size > 0) return (long long)st.st_size;
#ifdef __APPLE__
  uint64_t n = 0; uint32_t bs = 0;
  if (ioctl(fd, DKIOCGETBLOCKCOUNT, &n) == 0 && ioctl(fd, DKIOCGETBLOCKSIZE, &bs) == 0) return (long long)(n * bs);
#endif
#ifdef __linux__
  uint64_t b = 0;
  if (ioctl(fd, BLKGETSIZE64, &b) == 0) return (long long)b;
#endif
  return 0;
}

static int flush_fd(int fd) {
#ifdef __APPLE__
  if (fcntl(fd, F_FULLFSYNC) == 0) return 0;
#endif
  if (fsync(fd) == 0) return 0;
  /* a raw device may not take a flush: its writes were not cached anyway */
  if (errno == ENOTSUP || errno == ENOTTY || errno == EINVAL) return 0;
  return -1;
}

static int do_clone(const char *src, const char *dst) {
#ifdef __APPLE__
  return clonefile(src, dst, CLONE_NOFOLLOW);
#elif defined(__linux__)
  int s = open(src, O_RDONLY);
  if (s < 0) return -1;
  int d = open(dst, O_WRONLY | O_CREAT | O_EXCL, 0644);
  if (d < 0) { close(s); return -1; }
  int rc = ioctl(d, FICLONE, s);
  int e = errno;
  close(s); close(d);
  if (rc) { unlink(dst); errno = e; }
  return rc;
#else
  (void)src; (void)dst; errno = ENOTSUP; return -1;
#endif
}

static int cmd_fill(const char *path, long long mib) {
  int fd = open_w(path, O_WRONLY | O_CREAT | O_TRUNC, 0);
  size_t chunk = 16 << 20;
  char *b = abuf(chunk);
  double t0 = now();
  for (long long done = 0; done < mib; done += 16) {
    size_t n = (mib - done) >= 16 ? chunk : (size_t)(mib - done) << 20;
    b[0] ^= 1;
    if (write(fd, b, n) != (ssize_t)n) { perror("write"); return 1; }
  }
  if (flush_fd(fd)) { perror("flush"); return 1; }
  close(fd);
  printf("{\"mib\":%lld,\"seconds\":%.3f}\n", mib, now() - t0);
  return 0;
}

static int cmd_frag(int argc, char **argv) {
  /* frag FILE N SEED [MINKB MAXKB] [--size BYTES] */
  const char *path = argv[0];
  long long n = atoll(argv[1]);
  rng ^= (uint64_t)atoll(argv[2]) * 0x9E3779B97F4A7C15ULL;
  long long minkb = 4, maxkb = 64, size = 0;
  int pos = 0;
  for (int i = 3; i < argc; i++) {
    if (!strcmp(argv[i], "--size") && i + 1 < argc) size = atoll(argv[++i]);
    else if (pos == 0) { minkb = atoll(argv[i]); pos++; }
    else if (pos == 1) { maxkb = atoll(argv[i]); pos++; }
  }
  int fd = open_w(path, O_WRONLY, 0);
  if (!size) size = dev_size(fd);
  long long nblk = size / 4096, minb = minkb / 4, maxb = maxkb / 4;
  if (nblk < maxb + 1 || minb < 1 || maxb < minb) { fprintf(stderr, "frag: bad sizes\n"); return 2; }
  char *b = abuf((size_t)maxb * 4096);
  long long bytes = 0;
  double t0 = now();
  for (long long i = 0; i < n; i++) {
    long long len = minb + (long long)(xr() % (uint64_t)(maxb - minb + 1));
    long long at = (long long)(xr() % (uint64_t)(nblk - len));
    b[0] = (char)i;
    if (pwrite(fd, b, (size_t)len * 4096, at * 4096) != (ssize_t)(len * 4096)) { perror("pwrite"); return 1; }
    bytes += len * 4096;
  }
  if (flush_fd(fd)) { perror("flush"); return 1; }
  close(fd);
  printf("{\"writes\":%lld,\"bytes\":%lld,\"seconds\":%.3f}\n", n, bytes, now() - t0);
  return 0;
}

static int cmd_clone(const char *src, const char *dst) {
  double t0 = now();
  if (do_clone(src, dst)) { fprintf(stderr, "clone %s -> %s: %s\n", src, dst, strerror(errno)); return 1; }
  printf("{\"seconds\":%.6f}\n", now() - t0);
  return 0;
}

/* stall */
typedef struct { double s, e; } wr_t;
static wr_t *wrs;
static size_t nwr, capwr = 8u << 20;
static volatile int stop_flag;
static int wfd;
static long long wnblk;
static char *wbuf;
static int werr;

static void *writer(void *arg) {
  (void)arg;
  uint64_t r = 0x2545F4914F6CDD1DULL;
  while (!stop_flag) {
    r ^= r << 13; r ^= r >> 7; r ^= r << 17;
    long long at = (long long)(r % (uint64_t)wnblk);
    double s = now();
    if (pwrite(wfd, wbuf, 4096, at * 4096) != 4096) { werr = errno; break; }
    double e = now();
    if (nwr < capwr) { wrs[nwr].s = s; wrs[nwr].e = e; nwr++; }
  }
  return NULL;
}

static int cmpd(const void *a, const void *b) {
  double x = *(const double *)a, y = *(const double *)b;
  return x < y ? -1 : x > y;
}

static int cmd_stall(int argc, char **argv) {
  const char *wpath = NULL, *src = NULL, *dst = NULL;
  char **del = NULL;
  int ndel = 0, cached = 0, action = -1; /* 0 none, 1 clone, 2 delete */
  long long size = 0, region = 16LL << 20;
  double pre = 1.0, post = 1.0;
  for (int i = 0; i < argc; i++) {
    if (!strcmp(argv[i], "--write") && i + 1 < argc) wpath = argv[++i];
    else if (!strcmp(argv[i], "--size") && i + 1 < argc) size = atoll(argv[++i]);
    else if (!strcmp(argv[i], "--cached")) cached = 1;
    else if (!strcmp(argv[i], "--region") && i + 1 < argc) region = atoll(argv[++i]);
    else if (!strcmp(argv[i], "--pre") && i + 1 < argc) pre = atof(argv[++i]);
    else if (!strcmp(argv[i], "--post") && i + 1 < argc) post = atof(argv[++i]);
    else if (!strcmp(argv[i], "--none")) action = 0;
    else if (!strcmp(argv[i], "--clone") && i + 2 < argc) { action = 1; src = argv[i + 1]; dst = argv[i + 2]; i += 2; }
    else if (!strcmp(argv[i], "--delete")) { action = 2; del = argv + i + 1; ndel = argc - i - 1; break; }
  }
  if (!wpath || action < 0) { fprintf(stderr, "stall: needs --write and an action\n"); return 2; }
  wfd = open_w(wpath, O_WRONLY, cached);
  if (!size) size = dev_size(wfd);
  wnblk = size / 4096;
  /* the writer stays in the first REGION bytes, so that its own writes add at most
     REGION / 4096 pieces to the image the next measurement counts */
  if (region > 0 && region / 4096 < wnblk) wnblk = region / 4096;
  if (wnblk < 1) { fprintf(stderr, "stall: no size\n"); return 2; }
  wbuf = abuf(4096);
  wrs = malloc(capwr * sizeof(wr_t));
  pthread_t th;
  pthread_create(&th, NULL, writer, NULL);
  struct timespec ts = { (time_t)pre, (long)((pre - (long)pre) * 1e9) };
  nanosleep(&ts, NULL);
  double a = now();
  int rc = 0, e = 0;
  if (action == 1) { rc = do_clone(src, dst); e = errno; }
  else if (action == 2) { for (int i = 0; i < ndel; i++) if (unlink(del[i])) { rc = -1; e = errno; } }
  double b = now();
  ts.tv_sec = (time_t)post; ts.tv_nsec = (long)((post - (long)post) * 1e9);
  nanosleep(&ts, NULL);
  stop_flag = 1;
  pthread_join(th, NULL);
  close(wfd);
  if (werr) { fprintf(stderr, "stall: writer: %s\n", strerror(werr)); return 1; }
  if (rc) { fprintf(stderr, "stall: action failed: %s\n", strerror(e)); return 1; }
  double longest_during = 0, longest_any = 0;
  long long during = 0;
  size_t nbase = 0;
  double *base = malloc((nwr + 1) * sizeof(double));
  for (size_t i = 0; i < nwr; i++) {
    double l = wrs[i].e - wrs[i].s;
    if (l > longest_any) longest_any = l;
    if (wrs[i].s < b && wrs[i].e > a && l > longest_during) longest_during = l;
    if (wrs[i].e >= a && wrs[i].e <= b) during++;
    if (wrs[i].e < a) base[nbase++] = l;
  }
  /* a write that started before the action and is the one blocked: its wait counts from a */
  qsort(base, nbase, sizeof(double), cmpd);
  double p50 = nbase ? base[nbase / 2] : 0, p99 = nbase ? base[(size_t)(nbase * 0.99)] : 0;
  printf("{\"action_s\":%.6f,\"longest_during_s\":%.6f,\"longest_any_s\":%.6f,\"writes_during\":%lld,"
         "\"writes_total\":%zu,\"base_p50_s\":%.6f,\"base_p99_s\":%.6f,\"cached\":%s}\n",
         b - a, longest_during, longest_any, during, nwr, p50, p99, cached ? "true" : "false");
  return 0;
}

int main(int argc, char **argv) {
  if (argc >= 4 && !strcmp(argv[1], "fill")) return cmd_fill(argv[2], atoll(argv[3]));
  if (argc >= 5 && !strcmp(argv[1], "frag")) return cmd_frag(argc - 2, argv + 2);
  if (argc >= 4 && !strcmp(argv[1], "clone")) return cmd_clone(argv[2], argv[3]);
  if (argc >= 3 && !strcmp(argv[1], "stall")) return cmd_stall(argc - 2, argv + 2);
  fprintf(stderr, "usage: clonebench fill|frag|clone|stall ... (see the top of clonebench.c)\n");
  return 2;
}

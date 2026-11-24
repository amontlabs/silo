/* extents: how many pieces a file is in on the device, and where they are.
 *
 *   extents FILE          one JSON object: extents, data_bytes, data_ranges, calls, seconds
 *   extents --map FILE    one line per piece: "file_offset device_offset length" (bytes)
 *
 * A piece is a run of the file whose device offsets follow each other. On a Mac the map comes
 * from fcntl(F_LOG2PHYS_EXT), walked over the data ranges that lseek(SEEK_DATA/SEEK_HOLE) gives,
 * so a sparse disk image is not walked block by block. On Linux it comes from FIEMAP.
 * Adjacent answers whose device offsets touch are joined into one piece.
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

static double now(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec + t.tv_nsec / 1e9;
}

static int map_out;
static long long n_ext, n_calls, n_ranges, data_bytes;
static long long cur_log = -1, cur_dev = -1, cur_len = 0;

static void flush_piece(void) {
  if (cur_len > 0) {
    n_ext++;
    if (map_out) printf("%lld %lld %lld\n", cur_log, cur_dev, cur_len);
  }
  cur_len = 0;
}

/* one answer: file offset lo is at device offset dev, for len bytes */
static void piece(long long lo, long long dev, long long len) {
  data_bytes += len;
  if (cur_len > 0 && cur_log + cur_len == lo && cur_dev + cur_len == dev) {
    cur_len += len;
    return;
  }
  flush_piece();
  cur_log = lo; cur_dev = dev; cur_len = len;
}

#ifdef __APPLE__
static int walk(int fd, long long size) {
  long long off = 0;
  while (off < size) {
    off_t d = lseek(fd, off, SEEK_DATA);
    if (d < 0) {
      if (errno == ENXIO) break; /* no more data */
      if (errno == EINVAL || errno == ENOTSUP) d = off; /* no SEEK_DATA here: walk it all */
      else { perror("lseek SEEK_DATA"); return 1; }
    }
    off_t h = lseek(fd, d, SEEK_HOLE);
    if (h < 0) h = size;
    n_ranges++;
    long long p = d;
    while (p < h) {
      struct log2phys l;
      memset(&l, 0, sizeof l);
      l.l2p_contigbytes = h - p;
      l.l2p_devoffset = p;
      n_calls++;
      if (fcntl(fd, F_LOG2PHYS_EXT, &l) < 0) {
        /* not mapped (a hole inside a data range, or delayed allocation): step one block */
        flush_piece();
        p += 4096;
        continue;
      }
      if (l.l2p_contigbytes <= 0) { flush_piece(); p += 4096; continue; }
      long long len = l.l2p_contigbytes;
      if (len > h - p) len = h - p;
      if (l.l2p_devoffset < 0) {
        /* no space allocated there yet: XNU answers 0xfffffffffffff000 (Apple, filesystem-dev,
           July 2011); not a piece */
        flush_piece(); p += len; continue;
      }
      piece(p, l.l2p_devoffset, len);
      p += len;
    }
    flush_piece();
    off = h;
  }
  return 0;
}
#elif defined(__linux__)
static int walk(int fd, long long size) {
  enum { N = 4096 };
  size_t sz = sizeof(struct fiemap) + N * sizeof(struct fiemap_extent);
  struct fiemap *fm = calloc(1, sz);
  unsigned long long start = 0;
  long long last_end = -1;
  (void)size;
  for (;;) {
    memset(fm, 0, sz);
    fm->fm_start = start;
    fm->fm_length = ~0ULL - start;
    fm->fm_flags = FIEMAP_FLAG_SYNC;
    fm->fm_extent_count = N;
    n_calls++;
    if (ioctl(fd, FS_IOC_FIEMAP, fm) < 0) { perror("FIEMAP"); free(fm); return 1; }
    if (fm->fm_mapped_extents == 0) break;
    int last = 0;
    for (unsigned i = 0; i < fm->fm_mapped_extents; i++) {
      struct fiemap_extent *e = &fm->fm_extents[i];
      if ((long long)e->fe_logical != last_end) { flush_piece(); n_ranges++; }
      piece((long long)e->fe_logical, (long long)e->fe_physical, (long long)e->fe_length);
      last_end = (long long)(e->fe_logical + e->fe_length);
      if (e->fe_flags & FIEMAP_EXTENT_LAST) last = 1;
      start = e->fe_logical + e->fe_length;
    }
    if (last) break;
  }
  flush_piece();
  free(fm);
  return 0;
}
#else
static int walk(int fd, long long size) {
  (void)fd; (void)size;
  fprintf(stderr, "extents: not on this system (needs F_LOG2PHYS_EXT or FIEMAP)\n");
  return 3;
}
#endif

int main(int argc, char **argv) {
  const char *path = NULL;
  for (int i = 1; i < argc; i++) {
    if (!strcmp(argv[i], "--map")) map_out = 1;
    else path = argv[i];
  }
  if (!path) { fprintf(stderr, "usage: extents [--map] FILE\n"); return 2; }
  int fd = open(path, O_RDONLY);
  if (fd < 0) { perror(path); return 1; }
  struct stat st;
  if (fstat(fd, &st) < 0) { perror("fstat"); return 1; }
#ifdef __APPLE__
  fcntl(fd, F_FULLFSYNC); /* delayed allocations get their place first; errors do not matter */
#endif
  double t0 = now();
  int rc = walk(fd, (long long)st.st_size);
  double t1 = now();
  close(fd);
  if (rc) return rc;
  if (!map_out)
    printf("{\"extents\":%lld,\"data_bytes\":%lld,\"file_bytes\":%lld,\"data_ranges\":%lld,\"calls\":%lld,\"seconds\":%.4f}\n",
           n_ext, data_bytes, (long long)st.st_size, n_ranges, n_calls, t1 - t0);
  return 0;
}

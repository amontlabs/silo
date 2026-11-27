// Memory shared per person by construction: a person's forks map that person's memory snapshot
// copy-on-write (MAP_PRIVATE), and KSM stays off, so pages are shared between one person's forks
// and never with anyone else's.
// make <file> <secret>: writes a 256-page "snapshot" like ksm.c's victim (page k ends with
//   k == secret). fork <file>: maps it MAP_PRIVATE, reads every page, prints its pid, waits.
// probe <file>: Ben's 256 guesses, in a snapshot file of his own, mapped the same way; times one
//   write to each page, as ksm.c's probe does.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <time.h>
#include <sys/mman.h>
#define PG 4096
#define N 256
static double now() { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return t.tv_sec * 1e9 + t.tv_nsec; }
int main(int c, char **v) {
  if (!strcmp(v[1], "make")) {
    int s = atoi(v[3]); FILE *f = fopen(v[2], "w"); char p[PG];
    for (int k = 0; k < N; k++) { for (int i = 0; i < PG; i++) p[i] = (char)(i * 31 + 7); p[PG-2] = (char)k; p[PG-1] = (char)(s < 0 || k == s); fwrite(p, 1, PG, f); }
    fclose(f); return 0;
  }
  int fd = open(v[2], O_RDONLY); char *m = mmap(0, N * PG, PROT_READ|PROT_WRITE, MAP_PRIVATE, fd, 0);
  volatile char x = 0; for (int k = 0; k < N; k++) x += m[k*PG];
  if (!strcmp(v[1], "fork")) { printf("%d\n", getpid()); fflush(stdout); pause(); }
  double t[N]; int best = 0;
  for (int k = 0; k < N; k++) { double a = now(); m[k*PG] ^= 1; t[k] = now() - a; if (t[k] > t[best]) best = k; }
  double sum = 0; for (int k = 0; k < N; k++) if (k != best) sum += t[k];
  printf("slowest guess %d: %.0f ns; the other 255: %.0f ns on average\n", best, t[best], sum / 255);
  return 0;
}

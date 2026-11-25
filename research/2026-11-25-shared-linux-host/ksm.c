// Memory merging (KSM) between two members' processes, and what it tells one member about the other.
// victim <secret-byte>: fills 256 pages, each holding one 4 KiB "file page"; page k ends with
//   byte k == secret ? 1 : 0, so exactly one page is the "secret" one. Marks them mergeable and waits.
// probe: makes 256 guesses, one page per guess value, marks them mergeable, waits for KSM to
//   scan, then times one write to each page. A page that merged with the victim's takes a
//   copy-on-write fault and is slower. Prints the guess whose write was slowest, and the times.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <time.h>
#include <sys/mman.h>
#define PG 4096
static void fill(char *p, int k, int bit) { for (int i = 0; i < PG; i++) p[i] = (char)(i * 31 + 7); p[PG-2] = (char)k; p[PG-1] = (char)bit; }
static double now() { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return t.tv_sec * 1e9 + t.tv_nsec; }
int main(int c, char **v) {
  int n = 256; char *m = mmap(0, n * PG, PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
  if (!strcmp(v[1], "victim")) {
    int s = atoi(v[2]); for (int k = 0; k < n; k++) fill(m + k*PG, k, k == s);
    madvise(m, n*PG, MADV_MERGEABLE); printf("%d\n", getpid()); fflush(stdout); pause();
  }
  for (int k = 0; k < n; k++) fill(m + k*PG, k, 1);      // guess: "page k is the secret one"
  madvise(m, n*PG, MADV_MERGEABLE); sleep(atoi(v[2]));
  double t[256]; int best = 0;
  for (int k = 0; k < n; k++) { double a = now(); m[k*PG] ^= 1; t[k] = now() - a; if (t[k] > t[best]) best = k; }
  double sum = 0; for (int k = 0; k < n; k++) if (k != best) sum += t[k];
  printf("slowest guess %d: %.0f ns; the other 255: %.0f ns on average\n", best, t[best], sum / 255);
  return 0;
}

/* powerwatch: how long the machine slept, from a process that kept running across it.
 *
 *   powerwatch SECONDS_BETWEEN_SAMPLES [MAX_SECONDS]
 *
 * Prints one line per sample: wall time, the clock that counts sleep and the one that does not
 * (mach_continuous_time and mach_absolute_time on macOS, CLOCK_BOOTTIME and CLOCK_MONOTONIC on
 * Linux), both in seconds, and the sleep seen since the last sample: the growth of their
 * difference. A line "sleep S" follows a sample whose difference grew by more than a second. This
 * is how #370's helper would measure a sleep of the Mac without asking the system for its events.
 * Runs until killed, or MAX_SECONDS.
 */
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <sys/time.h>
#include <time.h>
#include <unistd.h>
#ifdef __APPLE__
#include <mach/mach_time.h>
static double ticks;
static void clocks(double *with_sleep, double *without) {
  *with_sleep = mach_continuous_time() * ticks;
  *without = mach_absolute_time() * ticks;
}
#else
static void clocks(double *with_sleep, double *without) {
  struct timespec a, b;
  clock_gettime(CLOCK_BOOTTIME, &a);
  clock_gettime(CLOCK_MONOTONIC, &b);
  *with_sleep = a.tv_sec + a.tv_nsec / 1e9;
  *without = b.tv_sec + b.tv_nsec / 1e9;
}
#endif

int main(int argc, char **argv) {
  if (argc < 2) { fprintf(stderr, "usage: powerwatch SECONDS_BETWEEN_SAMPLES [MAX_SECONDS]\n"); return 2; }
  double every = atof(argv[1]), max = argc > 2 ? atof(argv[2]) : 0;
#ifdef __APPLE__
  mach_timebase_info_data_t tb;
  mach_timebase_info(&tb);
  ticks = (double)tb.numer / tb.denom / 1e9;
#endif
  setvbuf(stdout, NULL, _IOLBF, 0);
  double w0, a0, prev = -1, start = -1;
  for (;;) {
    struct timeval tv;
    gettimeofday(&tv, NULL);
    clocks(&w0, &a0);
    double wall = tv.tv_sec + tv.tv_usec / 1e6, d = w0 - a0;
    if (start < 0) start = w0;
    printf("%.3f %.3f %.3f %.3f\n", wall, w0, a0, prev < 0 ? 0.0 : d - prev);
    if (prev >= 0 && d - prev > 1.0) printf("sleep %.1f\n", d - prev);
    prev = d;
    if (max > 0 && w0 - start >= max) return 0;
    usleep((useconds_t)(every * 1e6));
  }
}

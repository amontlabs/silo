/* clockagent: the guest's side of the clock channel, a stand-in for the
 * agent main would run on vsock. One command per line on the given tty:
 *   T          -> "T <realtime_ns> <monotonic_ns>"
 *   S <ns>     -> sets CLOCK_REALTIME to <ns>, then answers like T
 *   R          -> "R <rtc_epoch_s>"  (the guest's hardware clock, /dev/rtc0)
 * Built static on the host and put in the guest's initramfs. */
#define _GNU_SOURCE
#include <fcntl.h>
#include <linux/rtc.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <termios.h>
#include <time.h>
#include <unistd.h>

static long long ns(clockid_t c) {
  struct timespec t;
  clock_gettime(c, &t);
  return (long long)t.tv_sec * 1000000000LL + t.tv_nsec;
}

static long long rtc_now(void) {
  int fd = open("/dev/rtc0", O_RDONLY);
  if (fd < 0) return -1;
  struct rtc_time r;
  int ok = ioctl(fd, RTC_RD_TIME, &r);
  close(fd);
  if (ok < 0) return -1;
  struct tm tm = {.tm_sec = r.tm_sec, .tm_min = r.tm_min, .tm_hour = r.tm_hour,
                  .tm_mday = r.tm_mday, .tm_mon = r.tm_mon, .tm_year = r.tm_year};
  return (long long)timegm(&tm);
}

int main(int argc, char **argv) {
  const char *dev = argc > 1 ? argv[1] : "/dev/ttyS1";
  int fd = open(dev, O_RDWR | O_NOCTTY);
  if (fd < 0) { perror(dev); return 1; }
  struct termios t;
  if (tcgetattr(fd, &t) == 0) { cfmakeraw(&t); tcsetattr(fd, TCSANOW, &t); }
  FILE *in = fdopen(fd, "r");
  char line[128], out[128];
  while (fgets(line, sizeof line, in)) {
    if (line[0] == 'S') {
      long long v = atoll(line + 2);
      struct timespec ts = {v / 1000000000LL, v % 1000000000LL};
      clock_settime(CLOCK_REALTIME, &ts);
    }
    int n;
    if (line[0] == 'R')
      n = snprintf(out, sizeof out, "R %lld\n", rtc_now());
    else if (line[0] == 'T' || line[0] == 'S')
      n = snprintf(out, sizeof out, "T %lld %lld\n", ns(CLOCK_REALTIME), ns(CLOCK_MONOTONIC));
    else
      continue;
    if (write(fd, out, n) < 0) break;
  }
  return 0;
}

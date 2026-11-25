/* relay DOWNFD UPPATH BINDPATH: a helper's link as a relay. DOWNFD is the helper's end of the
 * socketpair whose other end the computer's network card holds; the relay binds BINDPATH,
 * connects to gvproxy's socket UPPATH, says "VFKT" and moves every frame between the two. On
 * SIGUSR1 (the helper started a new gvproxy) or when a send to gvproxy fails, it connects again
 * and says "VFKT" again. Frames that arrive while gvproxy is away are dropped and counted. */
#include <errno.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <sys/socket.h>
#include <sys/un.h>

static volatile sig_atomic_t want_reconnect;
static void on_usr1(int s) { (void)s; want_reconnect = 1; }

static double now(void) { struct timespec t; clock_gettime(CLOCK_REALTIME, &t); return t.tv_sec + t.tv_nsec / 1e9; }

static int connect_up(const char *up, const char *bindp) {
  int u = socket(AF_UNIX, SOCK_DGRAM, 0);
  int sz = 4 << 20; setsockopt(u, SOL_SOCKET, SO_RCVBUF, &sz, sizeof sz);
  sz = 1 << 20; setsockopt(u, SOL_SOCKET, SO_SNDBUF, &sz, sizeof sz);
  struct sockaddr_un a = { .sun_family = AF_UNIX };
  unlink(bindp);
  strncpy(a.sun_path, bindp, sizeof a.sun_path - 1);
  if (bind(u, (struct sockaddr *)&a, sizeof a) < 0) { close(u); return -1; }
  memset(a.sun_path, 0, sizeof a.sun_path);
  strncpy(a.sun_path, up, sizeof a.sun_path - 1);
  if (connect(u, (struct sockaddr *)&a, sizeof a) < 0 || send(u, "VFKT", 4, 0) < 0) { close(u); return -1; }
  return u;
}

int main(int argc, char **argv) {
  if (argc != 4) { fprintf(stderr, "usage: relay DOWNFD UPPATH BINDPATH\n"); return 2; }
  int down = atoi(argv[1]);
  signal(SIGUSR1, on_usr1);
  int up = -1; unsigned long dropped = 0, reconnects = 0;
  static char buf[65600];
  for (;;) {
    if (want_reconnect || up < 0) {
      want_reconnect = 0;
      if (up >= 0) close(up);
      up = connect_up(argv[2], argv[3]);
      if (up >= 0) { reconnects++; printf("{\"t\":%.3f,\"event\":\"connected\",\"n\":%lu,\"dropped\":%lu}\n", now(), reconnects, dropped); fflush(stdout); }
    }
    struct pollfd p[2] = { { down, POLLIN, 0 }, { up, POLLIN, 0 } };
    int r = poll(p, up >= 0 ? 2 : 1, up >= 0 ? -1 : 100);
    if (r < 0) continue; /* EINTR from SIGUSR1 */
    if (p[0].revents & POLLIN) {
      ssize_t n = recv(down, buf, sizeof buf, 0);
      if (n > 0) {
        if (up < 0 || send(up, buf, (size_t)n, 0) < 0) {
          dropped++;
          if (up >= 0 && (errno == ECONNREFUSED || errno == ENOENT || errno == ENOTCONN)) { close(up); up = -1; }
        }
      }
    }
    if (up >= 0 && (p[1].revents & (POLLIN | POLLERR))) {
      ssize_t n = recv(up, buf, sizeof buf, 0);
      if (n > 0) send(down, buf, (size_t)n, 0);
      else if (n < 0 && errno != EINTR && errno != EAGAIN) { close(up); up = -1; }
    }
  }
}

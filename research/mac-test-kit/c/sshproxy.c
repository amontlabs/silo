/* sshproxy: a stand-in for the small program Silo would give ssh as its ProxyCommand.
 *
 *   sshproxy [--fdpass] [--log FILE] unix:PATH | tcp:HOST:PORT
 *
 * It connects to the computer's way in (a Unix socket the network stack forwards to port 22
 * inside, or a TCP address), then either hands the connected socket to ssh and exits (--fdpass,
 * for "ProxyUseFdpass yes": one byte with SCM_RIGHTS on its standard output, as OpenSSH's
 * sshconnect.c expects), or relays bytes between its standard input and output and the socket
 * until either side closes.
 *
 * With --log it appends one JSON line to FILE: the time, its mode, the target, whether it worked,
 * and its own parent chain (its parent is ssh; ssh's parent is the program that started ssh, such
 * as Claude desktop or the Codex app). That chain is what Silo would show as "opened by".
 */
#ifdef __linux__
#define _GNU_SOURCE
#endif
#include <errno.h>
#include <netdb.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/types.h>
#include <sys/uio.h>
#include <sys/un.h>
#include <unistd.h>
#ifdef __APPLE__
#include <libproc.h>
#include <sys/proc_info.h>
#endif

static FILE *logf;
static char chainbuf[16384];

static void js(char *dst, size_t n, const char *s) {
  size_t k = strlen(dst);
  if (k + 2 >= n) return;
  dst[k++] = '"';
  for (; s && *s && k + 8 < n; s++) {
    unsigned char c = (unsigned char)*s;
    if (c == '"' || c == '\\') { dst[k++] = '\\'; dst[k++] = (char)c; }
    else if (c < 0x20) k += (size_t)snprintf(dst + k, n - k, "\\u%04x", c);
    else dst[k++] = (char)c;
  }
  dst[k++] = '"';
  dst[k] = 0;
}

static long one(long pid, char *dst, size_t n) {
  char path[4096] = "", comm[256] = "";
  long ppid = -1;
#ifdef __APPLE__
  struct proc_bsdinfo bi;
  if (proc_pidinfo((int)pid, PROC_PIDTBSDINFO, 0, &bi, sizeof bi) == (int)sizeof bi) {
    ppid = bi.pbi_ppid;
    snprintf(comm, sizeof comm, "%s", bi.pbi_name[0] ? bi.pbi_name : bi.pbi_comm);
  }
  if (proc_pidpath((int)pid, path, sizeof path) <= 0) path[0] = 0;
#else
  char f[64], buf[4096];
  snprintf(f, sizeof f, "/proc/%ld/stat", pid);
  FILE *fp = fopen(f, "r");
  if (fp) {
    size_t k = fread(buf, 1, sizeof buf - 1, fp);
    buf[k] = 0;
    fclose(fp);
    char *l = strchr(buf, '('), *r = strrchr(buf, ')');
    char st;
    if (l && r && r > l) {
      snprintf(comm, sizeof comm, "%.*s", (int)(r - l - 1), l + 1);
      if (sscanf(r + 2, "%c %ld", &st, &ppid) != 2) ppid = -1;
    }
  }
  snprintf(f, sizeof f, "/proc/%ld/exe", pid);
  ssize_t k = readlink(f, path, sizeof path - 1);
  path[k > 0 ? k : 0] = 0;
#endif
  size_t at = strlen(dst);
  snprintf(dst + at, n - at, "{\"pid\":%ld,\"ppid\":%ld,\"comm\":", pid, ppid);
  js(dst, n, comm);
  at = strlen(dst);
  snprintf(dst + at, n - at, ",\"path\":");
  js(dst, n, path);
  at = strlen(dst);
  snprintf(dst + at, n - at, "}");
  return ppid;
}

static void make_chain(void) {
  long pid = getpid();
  strcpy(chainbuf, "[");
  for (int i = 0; i < 40 && pid > 1; i++) {
    if (i) strcat(chainbuf, ",");
    long pp = one(pid, chainbuf, sizeof chainbuf);
    if (pp == pid) break;
    pid = pp;
  }
  strcat(chainbuf, "]");
}

static void logline(const char *mode, const char *target, int ok, const char *err) {
  if (!logf) return;
  struct timeval tv;
  gettimeofday(&tv, NULL);
  char b[20000] = "";
  snprintf(b, sizeof b, "{\"t\":%ld.%06ld,\"pid\":%d,\"mode\":\"%s\",\"target\":", (long)tv.tv_sec, (long)tv.tv_usec, (int)getpid(), mode);
  js(b, sizeof b, target);
  size_t at = strlen(b);
  snprintf(b + at, sizeof b - at, ",\"ok\":%s,\"error\":", ok ? "true" : "false");
  js(b, sizeof b, err);
  at = strlen(b);
  snprintf(b + at, sizeof b - at, ",\"chain\":%s}\n", chainbuf);
  fputs(b, logf);
  fflush(logf);
}

static int dial(const char *target, char *err, size_t errn) {
  if (!strncmp(target, "unix:", 5)) {
    struct sockaddr_un a;
    memset(&a, 0, sizeof a);
    a.sun_family = AF_UNIX;
    if (strlen(target + 5) >= sizeof a.sun_path) { snprintf(err, errn, "socket path too long"); return -1; }
    strcpy(a.sun_path, target + 5);
    int s = socket(AF_UNIX, SOCK_STREAM, 0);
    if (s < 0 || connect(s, (struct sockaddr *)&a, sizeof a) < 0) {
      snprintf(err, errn, "connect %s: %s", target + 5, strerror(errno));
      if (s >= 0) close(s);
      return -1;
    }
    return s;
  }
  if (!strncmp(target, "tcp:", 4)) {
    char host[256];
    snprintf(host, sizeof host, "%s", target + 4);
    char *colon = strrchr(host, ':');
    if (!colon) { snprintf(err, errn, "tcp:HOST:PORT expected"); return -1; }
    *colon = 0;
    struct addrinfo hints, *res, *ai;
    memset(&hints, 0, sizeof hints);
    hints.ai_socktype = SOCK_STREAM;
    int r = getaddrinfo(host, colon + 1, &hints, &res);
    if (r) { snprintf(err, errn, "getaddrinfo: %s", gai_strerror(r)); return -1; }
    int s = -1;
    for (ai = res; ai; ai = ai->ai_next) {
      s = socket(ai->ai_family, ai->ai_socktype, ai->ai_protocol);
      if (s < 0) continue;
      if (connect(s, ai->ai_addr, ai->ai_addrlen) == 0) break;
      snprintf(err, errn, "connect %s: %s", target + 4, strerror(errno));
      close(s);
      s = -1;
    }
    freeaddrinfo(res);
    return s;
  }
  snprintf(err, errn, "target must be unix:PATH or tcp:HOST:PORT");
  return -1;
}

static int pass_fd(int over, int fd) {
  struct msghdr m;
  struct iovec iov;
  char byte = 0;
  union { struct cmsghdr h; char buf[CMSG_SPACE(sizeof(int))]; } u;
  memset(&m, 0, sizeof m);
  memset(&u, 0, sizeof u);
  iov.iov_base = &byte;
  iov.iov_len = 1;
  m.msg_iov = &iov;
  m.msg_iovlen = 1;
  m.msg_control = u.buf;
  m.msg_controllen = sizeof u.buf;
  struct cmsghdr *c = CMSG_FIRSTHDR(&m);
  c->cmsg_level = SOL_SOCKET;
  c->cmsg_type = SCM_RIGHTS;
  c->cmsg_len = CMSG_LEN(sizeof(int));
  memcpy(CMSG_DATA(c), &fd, sizeof(int));
  ssize_t r;
  while ((r = sendmsg(over, &m, 0)) < 0 && errno == EINTR) {}
  return r == 1 ? 0 : -1;
}

static int relay(int s) {
  char buf[65536];
  struct pollfd p[2] = {{0, POLLIN, 0}, {s, POLLIN, 0}};
  int in_open = 1;
  for (;;) {
    p[0].fd = in_open ? 0 : -1;
    if (poll(p, 2, -1) < 0) { if (errno == EINTR) continue; return 1; }
    if (in_open && (p[0].revents & (POLLIN | POLLHUP))) {
      ssize_t n = read(0, buf, sizeof buf);
      if (n <= 0) { shutdown(s, SHUT_WR); in_open = 0; }
      else for (ssize_t o = 0; o < n;) { ssize_t w = write(s, buf + o, (size_t)(n - o)); if (w <= 0) return 1; o += w; }
    }
    if (p[1].revents & (POLLIN | POLLHUP | POLLERR)) {
      ssize_t n = read(s, buf, sizeof buf);
      if (n <= 0) return 0;
      for (ssize_t o = 0; o < n;) { ssize_t w = write(1, buf + o, (size_t)(n - o)); if (w <= 0) return 1; o += w; }
    }
  }
}

int main(int c, char **v) {
  int fdpass = 0;
  const char *target = NULL, *logpath = NULL;
  signal(SIGPIPE, SIG_IGN);
  for (int i = 1; i < c; i++) {
    if (!strcmp(v[i], "--fdpass")) fdpass = 1;
    else if (!strcmp(v[i], "--log") && i + 1 < c) logpath = v[++i];
    else target = v[i];
  }
  if (!target) { fprintf(stderr, "usage: sshproxy [--fdpass] [--log FILE] unix:PATH | tcp:HOST:PORT\n"); return 2; }
  if (logpath) logf = fopen(logpath, "a");
  make_chain();
  char err[512] = "";
  int s = dial(target, err, sizeof err);
  if (s < 0) {
    fprintf(stderr, "sshproxy: %s\n", err);
    logline(fdpass ? "fdpass" : "relay", target, 0, err);
    return 1;
  }
  if (fdpass) {
    if (pass_fd(1, s) != 0) {
      snprintf(err, sizeof err, "sendmsg on stdout: %s (is ProxyUseFdpass yes set?)", strerror(errno));
      fprintf(stderr, "sshproxy: %s\n", err);
      logline("fdpass", target, 0, err);
      return 1;
    }
    logline("fdpass", target, 1, "");
    return 0;
  }
  logline("relay", target, 1, "");
  return relay(s);
}

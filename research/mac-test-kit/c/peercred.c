/* peercred: what a Unix socket server learns about the process that connected to it.
 *
 *   peercred serve SOCKPATH [--max N] [--refuse-other-users] [--wait-ms MS]
 *       Listens on SOCKPATH (mode 0600 unless --mode is given before it: --mode 0777), accepts N
 *       connections (1 by default), and prints one JSON line per connection on stdout. For each
 *       connection it looks at the peer three times: at accept, after the client's first line,
 *       and after the client's second line or after --wait-ms when no second line comes (the
 *       client may have run exec in between). Each look holds every identity the system gives.
 *       --refuse-other-users answers "refused" to a peer whose user is not the server's, as the
 *       engine would, before reading anything else.
 *   peercred client SOCKPATH [--label TEXT] [--exec-self | --exec-sleep]
 *       Connects, sends "hello LABEL pid=N", waits for "ok" or "refused". With --exec-self it then
 *       runs exec on its own program, which sends "after-exec" on the same connection. With
 *       --exec-sleep it runs exec on /bin/sleep 3, which sends nothing but keeps the connection.
 *       Prints one JSON line with what the client saw, including its own audit token (macOS).
 *   peercred chain [PID]
 *       Prints the process's parent chain (default: itself) as JSON.
 *
 * macOS: getpeereid, LOCAL_PEERCRED (struct xucred), LOCAL_PEERPID, LOCAL_PEEREPID,
 * LOCAL_PEERUUID, LOCAL_PEERTOKEN (audit_token_t: val[5] pid, val[7] pid version, as XNU's
 * bsd/kern/kern_prot.c fills it), proc_pidinfo(PROC_PIDTBSDINFO) and proc_pidpath for the chain.
 * Linux: SO_PEERCRED, SO_PEERPIDFD (Linux 6.5), /proc for the chain.
 */
#ifdef __linux__
#define _GNU_SOURCE
#endif
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/types.h>
#include <sys/un.h>
#include <poll.h>
#include <unistd.h>
#ifdef __APPLE__
#include <libproc.h>
#include <mach/mach.h>
#include <sys/proc_info.h>
#include <sys/ucred.h>
#include <uuid/uuid.h>
#ifndef LOCAL_PEERTOKEN
#define LOCAL_PEERTOKEN 0x006
#endif
#ifndef LOCAL_PEEREPID
#define LOCAL_PEEREPID 0x003
#endif
#ifndef LOCAL_PEERUUID
#define LOCAL_PEERUUID 0x004
#endif
#endif
#ifdef __linux__
#ifndef SO_PEERPIDFD
#define SO_PEERPIDFD 77
#endif
#endif

#ifdef __linux__
/* glibc has no getpeereid; this is what it would return. */
static int getpeereid(int fd, uid_t *u, gid_t *g) {
  struct ucred uc;
  socklen_t l = sizeof uc;
  if (getsockopt(fd, SOL_SOCKET, SO_PEERCRED, &uc, &l) != 0) return -1;
  *u = uc.uid;
  *g = uc.gid;
  return 0;
}
#endif

static double now(void) {
  struct timeval tv;
  gettimeofday(&tv, NULL);
  return tv.tv_sec + tv.tv_usec / 1e6;
}

/* JSON output into a growing buffer. */
static char out[65536];
static size_t outn;
static void put(const char *s) {
  size_t n = strlen(s);
  if (outn + n < sizeof out - 1) { memcpy(out + outn, s, n); outn += n; out[outn] = 0; }
}
static void putf(const char *fmt, long long v) { char b[64]; snprintf(b, sizeof b, fmt, v); put(b); }
static void putstr(const char *s) {
  char b[8];
  put("\"");
  for (; s && *s; s++) {
    unsigned char c = (unsigned char)*s;
    if (c == '"' || c == '\\') { b[0] = '\\'; b[1] = (char)c; b[2] = 0; put(b); }
    else if (c < 0x20) { snprintf(b, sizeof b, "\\u%04x", c); put(b); }
    else { b[0] = (char)c; b[1] = 0; put(b); }
  }
  put("\"");
}
static void flush_line(FILE *f) { fprintf(f, "%s\n", out); fflush(f); outn = 0; out[0] = 0; }

/* One process: pid, ppid, uid, comm, path. Returns the parent pid, or -1. */
static long proc_one(long pid) {
  char path[4096] = "", comm[256] = "";
  long ppid = -1, uid = -1;
  int ok = 0;
#ifdef __APPLE__
  struct proc_bsdinfo bi;
  if (proc_pidinfo((int)pid, PROC_PIDTBSDINFO, 0, &bi, sizeof bi) == (int)sizeof bi) {
    ok = 1;
    ppid = bi.pbi_ppid;
    uid = bi.pbi_uid;
    snprintf(comm, sizeof comm, "%s", bi.pbi_name[0] ? bi.pbi_name : bi.pbi_comm);
  }
  if (proc_pidpath((int)pid, path, sizeof path) <= 0) path[0] = 0;
#else
  char f[64], buf[4096];
  snprintf(f, sizeof f, "/proc/%ld/stat", pid);
  FILE *fp = fopen(f, "r");
  if (fp) {
    size_t n = fread(buf, 1, sizeof buf - 1, fp);
    buf[n] = 0;
    fclose(fp);
    char *l = strchr(buf, '('), *r = strrchr(buf, ')');
    if (l && r && r > l) {
      snprintf(comm, sizeof comm, "%.*s", (int)(r - l - 1), l + 1);
      char st;
      if (sscanf(r + 2, "%c %ld", &st, &ppid) == 2) ok = 1;
    }
  }
  snprintf(f, sizeof f, "/proc/%ld/status", pid);
  fp = fopen(f, "r");
  if (fp) {
    while (fgets(buf, sizeof buf, fp))
      if (!strncmp(buf, "Uid:", 4)) { uid = strtol(buf + 4, NULL, 10); break; }
    fclose(fp);
  }
  snprintf(f, sizeof f, "/proc/%ld/exe", pid);
  ssize_t k = readlink(f, path, sizeof path - 1);
  path[k > 0 ? k : 0] = 0;
#endif
  put("{\"pid\":"); putf("%lld", pid);
  put(",\"ppid\":"); putf("%lld", ppid);
  put(",\"uid\":"); putf("%lld", uid);
  put(",\"comm\":"); putstr(comm);
  put(",\"path\":"); putstr(path);
  if (!ok) { put(",\"error\":"); putstr(strerror(errno)); }
  put("}");
  return ok ? ppid : -1;
}

static void chain(long pid) {
  put("[");
  for (int i = 0; i < 40 && pid > 0; i++) {
    if (i) put(",");
    long pp = proc_one(pid);
    if (pp == pid) break;
    pid = pp;
  }
  if (pid == 0) put(",{\"pid\":0}");
  put("]");
}

/* Everything the system says about the peer of fd, as one JSON object. */
static void look(int fd) {
  long pid = -1;
  put("{\"t\":"); { char b[32]; snprintf(b, sizeof b, "%.6f", now()); put(b); }
  uid_t eu; gid_t eg;
  if (getpeereid(fd, &eu, &eg) == 0) {
    put(",\"getpeereid\":{\"uid\":"); putf("%lld", eu); put(",\"gid\":"); putf("%lld", eg); put("}");
  } else { put(",\"getpeereid\":{\"error\":"); putstr(strerror(errno)); put("}"); }
#ifdef __APPLE__
  struct xucred xc;
  socklen_t l = sizeof xc;
  if (getsockopt(fd, SOL_LOCAL, LOCAL_PEERCRED, &xc, &l) == 0) {
    put(",\"local_peercred\":{\"version\":"); putf("%lld", xc.cr_version);
    put(",\"uid\":"); putf("%lld", xc.cr_uid);
    put(",\"groups\":[");
    for (int i = 0; i < xc.cr_ngroups && i < NGROUPS; i++) { if (i) put(","); putf("%lld", xc.cr_groups[i]); }
    put("]}");
  } else { put(",\"local_peercred\":{\"error\":"); putstr(strerror(errno)); put("}"); }
  pid_t p; l = sizeof p;
  if (getsockopt(fd, SOL_LOCAL, LOCAL_PEERPID, &p, &l) == 0) { pid = p; put(",\"local_peerpid\":"); putf("%lld", p); }
  else { put(",\"local_peerpid_error\":"); putstr(strerror(errno)); }
  l = sizeof p;
  if (getsockopt(fd, SOL_LOCAL, LOCAL_PEEREPID, &p, &l) == 0) { put(",\"local_peerepid\":"); putf("%lld", p); }
  else { put(",\"local_peerepid_error\":"); putstr(strerror(errno)); }
  uuid_t u; l = sizeof u;
  if (getsockopt(fd, SOL_LOCAL, LOCAL_PEERUUID, u, &l) == 0) {
    uuid_string_t us; uuid_unparse(u, us); put(",\"local_peeruuid\":"); putstr(us);
  } else { put(",\"local_peeruuid_error\":"); putstr(strerror(errno)); }
  audit_token_t tok; l = sizeof tok;
  if (getsockopt(fd, SOL_LOCAL, LOCAL_PEERTOKEN, &tok, &l) == 0) {
    put(",\"local_peertoken\":{\"auid\":"); putf("%lld", tok.val[0]);
    put(",\"euid\":"); putf("%lld", tok.val[1]);
    put(",\"egid\":"); putf("%lld", tok.val[2]);
    put(",\"ruid\":"); putf("%lld", tok.val[3]);
    put(",\"rgid\":"); putf("%lld", tok.val[4]);
    put(",\"pid\":"); putf("%lld", tok.val[5]);
    put(",\"asid\":"); putf("%lld", tok.val[6]);
    put(",\"pidversion\":"); putf("%lld", tok.val[7]);
    put("}");
  } else { put(",\"local_peertoken\":{\"error\":"); putstr(strerror(errno)); put("}"); }
#else
  struct ucred uc;
  socklen_t l = sizeof uc;
  if (getsockopt(fd, SOL_SOCKET, SO_PEERCRED, &uc, &l) == 0) {
    pid = uc.pid;
    put(",\"so_peercred\":{\"pid\":"); putf("%lld", uc.pid);
    put(",\"uid\":"); putf("%lld", uc.uid); put(",\"gid\":"); putf("%lld", uc.gid); put("}");
  } else { put(",\"so_peercred\":{\"error\":"); putstr(strerror(errno)); put("}"); }
  int pfd = -1; l = sizeof pfd;
  if (getsockopt(fd, SOL_SOCKET, SO_PEERPIDFD, &pfd, &l) == 0 && pfd >= 0) {
    char f[64], buf[4096]; long fp_pid = -1;
    snprintf(f, sizeof f, "/proc/self/fdinfo/%d", pfd);
    FILE *fp = fopen(f, "r");
    if (fp) {
      while (fgets(buf, sizeof buf, fp))
        if (!strncmp(buf, "Pid:", 4)) fp_pid = strtol(buf + 4, NULL, 10);
      fclose(fp);
    }
    put(",\"so_peerpidfd\":{\"pid\":"); putf("%lld", fp_pid); put("}");
    close(pfd);
  } else { put(",\"so_peerpidfd\":{\"error\":"); putstr(strerror(errno)); put("}"); }
#endif
  if (pid > 0) { put(",\"chain\":"); chain(pid); }
  put("}");
}

static int readline_fd(int fd, char *buf, size_t n, int timeout_ms) {
  size_t k = 0;
  while (k < n - 1) {
    struct pollfd p = {fd, POLLIN, 0};
    int r = poll(&p, 1, timeout_ms);
    if (r <= 0) break;
    ssize_t got = read(fd, buf + k, 1);
    if (got <= 0) break;
    if (buf[k] == '\n') break;
    k++;
  }
  buf[k] = 0;
  return (int)k;
}

static int serve(const char *path, int max, int refuse, int wait_ms, mode_t mode) {
  struct sockaddr_un a;
  if (strlen(path) >= sizeof a.sun_path) { fprintf(stderr, "socket path too long (%zu bytes, limit %zu)\n", strlen(path), sizeof a.sun_path - 1); return 2; }
  memset(&a, 0, sizeof a);
  a.sun_family = AF_UNIX;
  strcpy(a.sun_path, path);
  unlink(path);
  int s = socket(AF_UNIX, SOCK_STREAM, 0);
  if (s < 0 || bind(s, (struct sockaddr *)&a, sizeof a) < 0 || listen(s, 16) < 0) { perror("bind"); return 1; }
  chmod(path, mode);
  fprintf(stderr, "peercred: listening on %s, uid %d, pid %d\n", path, (int)geteuid(), (int)getpid());
  for (int n = 1; n <= max; n++) {
    int c = accept(s, NULL, NULL);
    if (c < 0) { perror("accept"); continue; }
    char line[512] = "";
    put("{\"event\":\"conn\",\"n\":"); putf("%lld", n);
    put(",\"server_uid\":"); putf("%lld", geteuid());
    put(",\"at_accept\":"); look(c);
    uid_t eu; gid_t eg;
    int other = getpeereid(c, &eu, &eg) != 0 || eu != geteuid();
    if (refuse && other) {
      (void)!write(c, "refused\n", 8);
      put(",\"refused\":true}");
      flush_line(stdout);
      close(c);
      continue;
    }
    readline_fd(c, line, sizeof line, 5000);
    put(",\"hello\":"); putstr(line);
    put(",\"after_hello\":"); look(c);
    (void)!write(c, "ok\n", 3);
    line[0] = 0;
    int got = readline_fd(c, line, sizeof line, wait_ms);
    (void)got;
    put(",\"second\":"); putstr(line);
    put(",\"later\":"); look(c);
    put(",\"refused\":false}");
    flush_line(stdout);
    (void)!write(c, "bye\n", 4);
    close(c);
  }
  close(s);
  unlink(path);
  return 0;
}

static void own_token(void) {
#ifdef __APPLE__
  audit_token_t tok;
  mach_msg_type_number_t cnt = TASK_AUDIT_TOKEN_COUNT;
  if (task_info(mach_task_self(), TASK_AUDIT_TOKEN, (task_info_t)&tok, &cnt) == KERN_SUCCESS) {
    put(",\"own_token\":{\"pid\":"); putf("%lld", tok.val[5]); put(",\"pidversion\":"); putf("%lld", tok.val[7]); put("}");
  }
#endif
}

static int self_path(char *buf, size_t n) {
#ifdef __APPLE__
  return proc_pidpath(getpid(), buf, (uint32_t)n) > 0 ? 0 : -1;
#else
  ssize_t k = readlink("/proc/self/exe", buf, n - 1);
  if (k <= 0) return -1;
  buf[k] = 0;
  return 0;
#endif
}

static int client(const char *path, const char *label, int exec_mode) {
  struct sockaddr_un a;
  memset(&a, 0, sizeof a);
  a.sun_family = AF_UNIX;
  snprintf(a.sun_path, sizeof a.sun_path, "%s", path);
  int s = socket(AF_UNIX, SOCK_STREAM, 0);
  put("{\"event\":\"client\",\"label\":"); putstr(label);
  put(",\"pid\":"); putf("%lld", getpid()); put(",\"uid\":"); putf("%lld", geteuid());
  own_token();
  if (connect(s, (struct sockaddr *)&a, sizeof a) < 0) {
    put(",\"connect\":"); putstr(strerror(errno)); put("}");
    flush_line(stdout);
    return 3;
  }
  put(",\"connect\":\"ok\"");
  char msg[300];
  snprintf(msg, sizeof msg, "hello %s pid=%d\n", label, (int)getpid());
  (void)!write(s, msg, strlen(msg));
  char line[64] = "";
  readline_fd(s, line, sizeof line, 10000);
  put(",\"answer\":"); putstr(line);
  if (strcmp(line, "ok") || !exec_mode) {
    readline_fd(s, line, sizeof line, 300);
    put("}");
    flush_line(stdout);
    return strcmp(line, "refused") ? 0 : 4;
  }
  put(",\"exec\":"); putstr(exec_mode == 1 ? "self" : "sleep"); put("}");
  flush_line(stdout);
  char fdarg[16];
  snprintf(fdarg, sizeof fdarg, "%d", s);
  int fl = fcntl(s, F_GETFD);
  fcntl(s, F_SETFD, fl & ~FD_CLOEXEC);
  if (exec_mode == 1) {
    char me[4096];
    if (self_path(me, sizeof me) != 0) { perror("own path"); return 1; }
    execl(me, me, "after-exec", fdarg, label, (char *)NULL);
  } else {
    execl("/bin/sleep", "sleep", "3", (char *)NULL);
  }
  perror("exec");
  return 1;
}

static int after_exec(int fd, const char *label) {
  put("{\"event\":\"after-exec\",\"label\":"); putstr(label);
  put(",\"pid\":"); putf("%lld", getpid());
  own_token();
  (void)!write(fd, "after-exec\n", 11);
  char line[64] = "";
  readline_fd(fd, line, sizeof line, 10000);
  put(",\"answer\":"); putstr(line); put("}");
  flush_line(stdout);
  return 0;
}

int main(int c, char **v) {
  signal(SIGPIPE, SIG_IGN);
  if (c >= 3 && !strcmp(v[1], "serve")) {
    int max = 1, refuse = 0, wait_ms = 800;
    mode_t mode = 0600;
    for (int i = 3; i < c; i++) {
      if (!strcmp(v[i], "--max") && i + 1 < c) max = atoi(v[++i]);
      else if (!strcmp(v[i], "--refuse-other-users")) refuse = 1;
      else if (!strcmp(v[i], "--wait-ms") && i + 1 < c) wait_ms = atoi(v[++i]);
      else if (!strcmp(v[i], "--mode") && i + 1 < c) mode = (mode_t)strtol(v[++i], NULL, 8);
    }
    return serve(v[2], max, refuse, wait_ms, mode);
  }
  if (c >= 3 && !strcmp(v[1], "client")) {
    const char *label = "client";
    int ex = 0;
    for (int i = 3; i < c; i++) {
      if (!strcmp(v[i], "--label") && i + 1 < c) label = v[++i];
      else if (!strcmp(v[i], "--exec-self")) ex = 1;
      else if (!strcmp(v[i], "--exec-sleep")) ex = 2;
    }
    return client(v[2], label, ex);
  }
  if (c >= 4 && !strcmp(v[1], "after-exec")) return after_exec(atoi(v[2]), v[3]);
  if (c >= 2 && !strcmp(v[1], "chain")) {
    put("{\"chain\":"); chain(c >= 3 ? atol(v[2]) : (long)getpid()); put("}");
    flush_line(stdout);
    return 0;
  }
  fprintf(stderr, "usage: peercred serve SOCK [--max N] [--refuse-other-users] [--wait-ms MS] [--mode OCTAL]\n"
                  "       peercred client SOCK [--label TEXT] [--exec-self | --exec-sleep]\n"
                  "       peercred chain [PID]\n");
  return 2;
}

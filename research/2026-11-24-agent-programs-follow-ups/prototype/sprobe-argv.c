// A stand-in for Silo's probe, built on seccomp user notification: sprobe starts the agent program
// under a filter that sends its process starts, its opens for writing, its unlinks and renames to
// sprobe. While armed, sprobe makes those fail with EPERM, apart from writes under an allowed
// prefix. Commands come one per line from a FIFO: "allow <prefix>", "arm", "disarm".
// Usage: sprobe <ctl-fifo> <log-file> -- <program> [args...]
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <poll.h>
#include <seccomp.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <linux/seccomp.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>

static FILE *logf_;
static char allow[8][PATH_MAX]; static int nallow; static int armed;
static char allowargv[8][PATH_MAX]; static int nallowargv; static int oneshot;
static long long counts[2];
static int ackfd = -1; // notifications seen while not armed / armed

static void logm(const char *fmt, const char *a, int pid) {
    struct timespec ts; clock_gettime(CLOCK_REALTIME, &ts);
    fprintf(logf_, "%ld.%03ld ", ts.tv_sec, ts.tv_nsec / 1000000); fprintf(logf_, fmt, a, pid); fputc('\n', logf_); fflush(logf_);
}

static int send_fd(int sock, int fd) {
    char buf[CMSG_SPACE(sizeof(int))] = {0}; char c = 'x';
    struct iovec io = {&c, 1}; struct msghdr m = {0}; m.msg_iov = &io; m.msg_iovlen = 1; m.msg_control = buf; m.msg_controllen = sizeof buf;
    struct cmsghdr *cm = CMSG_FIRSTHDR(&m); cm->cmsg_level = SOL_SOCKET; cm->cmsg_type = SCM_RIGHTS; cm->cmsg_len = CMSG_LEN(sizeof(int));
    memcpy(CMSG_DATA(cm), &fd, sizeof(int)); return sendmsg(sock, &m, 0);
}
static int recv_fd(int sock) {
    char buf[CMSG_SPACE(sizeof(int))] = {0}; char c;
    struct iovec io = {&c, 1}; struct msghdr m = {0}; m.msg_iov = &io; m.msg_iovlen = 1; m.msg_control = buf; m.msg_controllen = sizeof buf;
    if (recvmsg(sock, &m, 0) <= 0) return -1; int fd; memcpy(&fd, CMSG_DATA(CMSG_FIRSTHDR(&m)), sizeof(int)); return fd;
}

static void rule(scmp_filter_ctx ctx, int sc) { seccomp_rule_add(ctx, SCMP_ACT_NOTIFY, sc, 0); }
static void rule_flags(scmp_filter_ctx ctx, int sc, int argi) {
    int bits[] = {O_WRONLY, O_RDWR, O_CREAT, O_TRUNC};
    for (int i = 0; i < 4; i++) seccomp_rule_add(ctx, SCMP_ACT_NOTIFY, sc, 1, SCMP_CMP(argi, SCMP_CMP_MASKED_EQ, bits[i], bits[i]));
}

static int read_str(int pid, unsigned long addr, char *out, size_t n) {
    char p[64]; snprintf(p, sizeof p, "/proc/%d/mem", pid);
    int fd = open(p, O_RDONLY); if (fd < 0) return -1;
    ssize_t r = pread(fd, out, n - 1, addr); close(fd); if (r <= 0) return -1; out[r] = 0; out[strnlen(out, r)] = 0; return 0;
}

static void resolve(int pid, int dirfd, const char *rel, char *out) {
    if (rel[0] == '/') {
        // Follow /proc/self/fd/N/... and /proc/<pid>/fd/N/..., which agent programs use to write.
        char tmp[PATH_MAX]; const char *p = rel; int fdn = -1, used = 0;
        if (!strncmp(p, "/proc/self/fd/", 14) || !strncmp(p, "/proc/thread-self/fd/", 21)) { p = strstr(p, "/fd/") + 4; sscanf(p, "%d%n", &fdn, &used); }
        else if (!strncmp(p, "/proc/", 6)) { int qp; if (sscanf(p + 6, "%d/fd/%d%n", &qp, &fdn, &used) == 2) { p = strstr(p + 6, "/fd/") + 4; sscanf(p, "%d%n", &fdn, &used); } }
        if (fdn >= 0) {
            char link[64], base[PATH_MAX] = ""; snprintf(link, sizeof link, "/proc/%d/fd/%d", pid, fdn);
            ssize_t r = readlink(link, base, sizeof base - 1); if (r > 0) base[r] = 0;
            snprintf(tmp, sizeof tmp, "%s%s", base, p + used); snprintf(out, PATH_MAX, "%s", tmp); return;
        }
        snprintf(out, PATH_MAX, "%s", rel); return;
    }
    char link[64], base[PATH_MAX] = "";
    if (dirfd == AT_FDCWD) snprintf(link, sizeof link, "/proc/%d/cwd", pid); else snprintf(link, sizeof link, "/proc/%d/fd/%d", pid, dirfd);
    ssize_t r = readlink(link, base, sizeof base - 1); if (r > 0) base[r] = 0;
    snprintf(out, PATH_MAX, "%s/%s", base, rel);
}

// argv of an exec, joined with spaces, read from the caller's memory.
static void read_argv(int pid, unsigned long argvp, char *out, size_t n) {
    char p[64]; snprintf(p, sizeof p, "/proc/%d/mem", pid); out[0] = 0;
    int fd = open(p, O_RDONLY); if (fd < 0) return;
    size_t used = 0;
    for (int i = 0; i < 64 && used < n - 2; i++) {
        unsigned long ptr = 0; if (pread(fd, &ptr, sizeof ptr, argvp + i * sizeof ptr) != sizeof ptr || !ptr) break;
        char s[1024]; ssize_t r = pread(fd, s, sizeof s - 1, ptr); if (r <= 0) break; s[r] = 0;
        int k = snprintf(out + used, n - used, "%s%s", i ? " " : "", s); if (k < 0) break; used += k; if (used >= n) { used = n - 1; break; }
    }
    close(fd);
}

static int allowed_argv(const char *argv) {
    for (int i = 0; i < nallowargv; i++) if (!strncmp(argv, allowargv[i], strlen(allowargv[i]))) return 1;
    return 0;
}

static int allowed(const char *path) {
    for (int i = 0; i < nallow; i++) if (!strncmp(path, allow[i], strlen(allow[i]))) return 1;
    return 0;
}

int main(int argc, char **argv) {
    if (argc < 5 || strcmp(argv[3], "--")) { fprintf(stderr, "usage\n"); return 2; }
    logf_ = fopen(argv[2], "a");
    int ctl = open(argv[1], O_RDONLY | O_NONBLOCK);
    if (getenv("SPROBE_ACK")) ackfd = open(getenv("SPROBE_ACK"), O_RDWR);
    int sv[2]; socketpair(AF_UNIX, SOCK_STREAM, 0, sv);
    pid_t child = fork();
    if (child == 0) {
        close(sv[0]);
        scmp_filter_ctx ctx = seccomp_init(SCMP_ACT_ALLOW);
        rule(ctx, SCMP_SYS(execve)); rule(ctx, SCMP_SYS(execveat));
        rule_flags(ctx, SCMP_SYS(openat), 2); rule_flags(ctx, SCMP_SYS(open), 1); rule(ctx, SCMP_SYS(creat)); rule(ctx, SCMP_SYS(openat2));
        rule(ctx, SCMP_SYS(unlink)); rule(ctx, SCMP_SYS(unlinkat)); rule(ctx, SCMP_SYS(rename)); rule(ctx, SCMP_SYS(renameat)); rule(ctx, SCMP_SYS(renameat2));
        rule(ctx, SCMP_SYS(truncate));
        if (seccomp_load(ctx)) { perror("seccomp_load"); _exit(3); }
        int nfd = seccomp_notify_fd(ctx); send_fd(sv[1], nfd); close(nfd); close(sv[1]);
        execvp(argv[4], argv + 4); perror("exec"); _exit(4);
    }
    close(sv[1]);
    int nfd = recv_fd(sv[0]);
    logm("sprobe: watching %s, pid %d", argv[4], child);
    struct seccomp_notif *req; struct seccomp_notif_resp *resp; seccomp_notify_alloc(&req, &resp);
    char line[PATH_MAX + 16]; size_t ll = 0;
    for (;;) {
        struct pollfd pf[2] = {{nfd, POLLIN, 0}, {ctl, POLLIN, 0}};
        int pr = poll(pf, 2, 200);
        int st; if (waitpid(child, &st, WNOHANG) == child) break;
        if (pr <= 0) continue;
        if (pf[1].revents & POLLIN) {
            char c; while (read(ctl, &c, 1) == 1) {
                if (c != '\n') { if (ll < sizeof line - 1) line[ll++] = c; continue; }
                line[ll] = 0; ll = 0;
                if (!strncmp(line, "allow ", 6) && nallow < 8) snprintf(allow[nallow++], PATH_MAX, "%s", line + 6);
                else if (!strncmp(line, "allowargv ", 10) && nallowargv < 8) snprintf(allowargv[nallowargv++], PATH_MAX, "%s", line + 10);
                else if (!strcmp(line, "oneshot")) oneshot = 1;
                else if (!strcmp(line, "reset")) { nallow = 0; nallowargv = 0; oneshot = 0; } else if (!strcmp(line, "arm")) armed = 1; else if (!strcmp(line, "disarm")) armed = 0;
                logm("sprobe: command %s%.0d", line, 0);
                if (ackfd >= 0 && (!strcmp(line, "arm") || !strcmp(line, "disarm"))) { char a[64]; int k = snprintf(a, sizeof a, "ok %s\n", line); if (write(ackfd, a, k) < 0) {} }
            }
        }
        if (pf[1].revents & POLLHUP) { close(ctl); ctl = open(argv[1], O_RDONLY | O_NONBLOCK); }
        if (!(pf[0].revents & POLLIN)) continue;
        memset(req, 0, sizeof *req);
        if (seccomp_notify_receive(nfd, req)) continue;
        memset(resp, 0, sizeof *resp); resp->id = req->id;
        int deny = 0; char raw[PATH_MAX] = "", path[PATH_MAX] = ""; const char *what = "";
        int nr = req->data.nr; unsigned long *a = (unsigned long *)req->data.args;
        counts[armed]++;
        if (armed) {
            if (nr == SCMP_SYS(execve) || nr == SCMP_SYS(execveat)) {
                what = "exec"; char av[PATH_MAX]; read_str(req->pid, nr == SCMP_SYS(execve) ? a[0] : a[1], raw, sizeof raw);
                read_argv(req->pid, nr == SCMP_SYS(execve) ? a[1] : a[2], av, sizeof av);
                snprintf(path, sizeof path, "%s [%.3000s]", raw, av); deny = !allowed_argv(av);
                if (!deny) logm("ALLOW exec by argv: %s%.0d", path, 0);
            }
            else {
                int dirfd = AT_FDCWD; unsigned long p = a[0];
                if (nr == SCMP_SYS(openat) || nr == SCMP_SYS(openat2) || nr == SCMP_SYS(unlinkat) || nr == SCMP_SYS(renameat) || nr == SCMP_SYS(renameat2)) { dirfd = (int)a[0]; p = a[1]; }
                read_str(req->pid, p, raw, sizeof raw); resolve(req->pid, dirfd, raw, path);
                what = (nr == SCMP_SYS(unlink) || nr == SCMP_SYS(unlinkat)) ? "unlink" : (nr == SCMP_SYS(rename) || nr == SCMP_SYS(renameat) || nr == SCMP_SYS(renameat2)) ? "rename" : "write-open";
                deny = !allowed(path);
            }
        }
        if (deny) { resp->error = -EPERM; char m[PATH_MAX + 32]; snprintf(m, sizeof m, "DENY %s %s", what, path); logm("%s by pid %d", m, req->pid);
            if (oneshot) { armed = 0; logm("sprobe: one-shot, disarmed after the first refusal%s%.0d", "", 0); } }
        else resp->flags = SECCOMP_USER_NOTIF_FLAG_CONTINUE;
        seccomp_notify_respond(nfd, resp);
    }
    char m[128]; snprintf(m, sizeof m, "sprobe: done; notifications unarmed=%lld armed=%lld", counts[0], counts[1]); logm("%s%.0d", m, 0);
    int st; waitpid(child, &st, 0); return WIFEXITED(st) ? WEXITSTATUS(st) : 1;
}

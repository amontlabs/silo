/* vsockrelay: the relay a computer runs when its network card leads nowhere (#378), for tests 40
 * and 43. ./kit build compiles it on the Mac; the tests copy it into the computer, which runs the
 * same macOS, so the same binary runs there.
 *
 *   vsockrelay out --listen 127.0.0.1:3128 --port 3128 [--cid 2] [--unix PATH]
 *       In the computer: a TCP listener on the computer's own loopback. Each connection is carried
 *       to vsock port PORT of CID (2, the Mac), where silovm's --vsock-out hands it to Silo's proxy.
 *   vsockrelay in --port 22 --to 127.0.0.1:22 [--unix PATH]
 *       In the computer: a vsock listener on PORT. Each connection the Mac opens (silovm's
 *       --vsock-in) is carried to the TCP address TO: the computer's own SSH server.
 *   --unix PATH replaces the vsock side with a Unix socket (connected for out, listened on for in):
 *       the Linux analogue, where a network namespace stands for the computer and a Unix socket for
 *       vsock, as #378 measured.
 *   --once  serve one connection and exit (the tests' self-checks).
 * Each connection is a child process; the parent never exits on a failed connection. One line per
 * connection on stderr. */
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <errno.h>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <unistd.h>
#if defined(__APPLE__)
#include <sys/vsock.h>
#define HAVE_VSOCK 1
#elif defined(__linux__)
#include <linux/vm_sockets.h>
#define HAVE_VSOCK 1
#endif

static void die(const char *what) { fprintf(stderr, "vsockrelay: %s: %s\n", what, strerror(errno)); exit(1); }
static void usage(void) {
	fprintf(stderr, "usage: vsockrelay out --listen HOST:PORT --port N [--cid N] [--unix PATH] [--once]\n"
	                "       vsockrelay in --port N --to HOST:PORT [--unix PATH] [--once]\n");
	exit(2);
}

static int parse_inet(const char *s, struct sockaddr_in *a) {
	char host[64];
	const char *c = strrchr(s, ':');
	if (!c || (size_t)(c - s) >= sizeof host) return -1;
	memcpy(host, s, c - s); host[c - s] = 0;
	memset(a, 0, sizeof *a);
	a->sin_family = AF_INET;
	a->sin_port = htons((unsigned short)atoi(c + 1));
	return inet_pton(AF_INET, host, &a->sin_addr) == 1 ? 0 : -1;
}

static int unix_addr(const char *path, struct sockaddr_un *a) {
	memset(a, 0, sizeof *a);
	a->sun_family = AF_UNIX;
	if (strlen(path) >= sizeof a->sun_path) return -1;
	strcpy(a->sun_path, path);
	return 0;
}

#ifdef HAVE_VSOCK
static void vsock_addr(struct sockaddr_vm *a, unsigned cid, unsigned port) {
	memset(a, 0, sizeof *a);
#ifdef __APPLE__
	a->svm_len = sizeof *a;
#endif
	a->svm_family = AF_VSOCK;
	a->svm_cid = cid;
	a->svm_port = port;
}
#endif

/* Copies both ways until both ends are done; an end is passed on as a shutdown of writing. */
static void pump(int a, int b) {
	char buf[65536];
	int open_a = 1, open_b = 1;
	while (open_a || open_b) {
		struct pollfd p[2] = {{a, open_a ? POLLIN : 0, 0}, {b, open_b ? POLLIN : 0, 0}};
		if (poll(p, 2, -1) < 0) { if (errno == EINTR) continue; return; }
		for (int i = 0; i < 2; i++) {
			int from = i ? b : a, to = i ? a : b;
			int *open = i ? &open_b : &open_a;
			if (!*open || !(p[i].revents & (POLLIN | POLLHUP | POLLERR))) continue;
			ssize_t n = read(from, buf, sizeof buf);
			if (n <= 0) { *open = 0; shutdown(to, SHUT_WR); continue; }
			for (ssize_t off = 0; off < n;) {
				ssize_t w = write(to, buf + off, n - off);
				if (w < 0 && errno == EINTR) continue;
				if (w <= 0) return;
				off += w;
			}
		}
	}
}

int main(int argc, char **argv) {
	if (argc < 2) usage();
	int out = !strcmp(argv[1], "out"), in = !strcmp(argv[1], "in");
	if (!out && !in) usage();
	const char *listen_s = 0, *to_s = 0, *unix_path = 0;
	unsigned port = 0, cid = 2;
	int once = 0;
	for (int i = 2; i < argc; i++) {
		if (!strcmp(argv[i], "--once")) { once = 1; continue; }
		if (i + 1 >= argc) usage();
		if (!strcmp(argv[i], "--listen")) listen_s = argv[++i];
		else if (!strcmp(argv[i], "--to")) to_s = argv[++i];
		else if (!strcmp(argv[i], "--unix")) unix_path = argv[++i];
		else if (!strcmp(argv[i], "--port")) port = (unsigned)atoi(argv[++i]);
		else if (!strcmp(argv[i], "--cid")) cid = (unsigned)atoi(argv[++i]);
		else usage();
	}
	if (out && (!listen_s || (!port && !unix_path))) usage();
	if (in && (!to_s || (!port && !unix_path))) usage();
#ifndef HAVE_VSOCK
	if (!unix_path) { fprintf(stderr, "vsockrelay: no vsock on this system; use --unix\n"); return 2; }
#endif
	signal(SIGCHLD, SIG_IGN);
	signal(SIGPIPE, SIG_IGN);

	/* The listening side: TCP for out, vsock (or the Unix socket) for in. */
	int lfd;
	if (out) {
		struct sockaddr_in a;
		if (parse_inet(listen_s, &a)) { fprintf(stderr, "vsockrelay: bad --listen %s\n", listen_s); return 2; }
		lfd = socket(AF_INET, SOCK_STREAM, 0);
		int one = 1;
		setsockopt(lfd, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
		if (lfd < 0 || bind(lfd, (struct sockaddr *)&a, sizeof a)) die("bind --listen");
	} else if (unix_path) {
		struct sockaddr_un a;
		if (unix_addr(unix_path, &a)) { fprintf(stderr, "vsockrelay: --unix path too long\n"); return 2; }
		unlink(unix_path);
		lfd = socket(AF_UNIX, SOCK_STREAM, 0);
		if (lfd < 0 || bind(lfd, (struct sockaddr *)&a, sizeof a)) die("bind --unix");
	} else {
#ifdef HAVE_VSOCK
		struct sockaddr_vm a;
		vsock_addr(&a, VMADDR_CID_ANY, port);
		lfd = socket(AF_VSOCK, SOCK_STREAM, 0);
		if (lfd < 0 || bind(lfd, (struct sockaddr *)&a, sizeof a)) die("bind vsock");
#else
		return 2;
#endif
	}
	if (listen(lfd, 64)) die("listen");
	fprintf(stderr, "vsockrelay: %s ready\n", out ? "out" : "in");

	for (;;) {
		int c = accept(lfd, 0, 0);
		if (c < 0) { if (errno == EINTR) continue; die("accept"); }
		pid_t pid = once ? 0 : fork();
		if (pid < 0) { close(c); continue; }
		if (pid > 0) { close(c); continue; }
		if (!once) close(lfd);
		int d = -1;
		if (out && unix_path) {
			struct sockaddr_un a; unix_addr(unix_path, &a);
			d = socket(AF_UNIX, SOCK_STREAM, 0);
			if (connect(d, (struct sockaddr *)&a, sizeof a)) { fprintf(stderr, "vsockrelay: connect %s: %s\n", unix_path, strerror(errno)); _exit(1); }
		} else if (out) {
#ifdef HAVE_VSOCK
			struct sockaddr_vm a;
			vsock_addr(&a, cid, port);
			d = socket(AF_VSOCK, SOCK_STREAM, 0);
			if (connect(d, (struct sockaddr *)&a, sizeof a)) { fprintf(stderr, "vsockrelay: connect vsock %u:%u: %s\n", cid, port, strerror(errno)); _exit(1); }
#endif
		} else {
			struct sockaddr_in a;
			parse_inet(to_s, &a);
			d = socket(AF_INET, SOCK_STREAM, 0);
			int one = 1;
			setsockopt(d, IPPROTO_TCP, TCP_NODELAY, &one, sizeof one);
			if (connect(d, (struct sockaddr *)&a, sizeof a)) { fprintf(stderr, "vsockrelay: connect %s: %s\n", to_s, strerror(errno)); _exit(1); }
		}
		fprintf(stderr, "vsockrelay: connection carried\n");
		pump(c, d);
		close(c); close(d);
		if (once) return 0;
		_exit(0);
	}
}

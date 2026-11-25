/* nicpump TAPFD SOCKFD: move frames between a TAP device and a datagram socket, both inherited,
 * until killed. The data path of guest-nic.py, in C so that the stand-in for the computer's
 * network card is not what limits the throughput measured. A failed send is counted, never
 * retried, as a network card drops a frame it cannot pass: the count goes to stderr. */
#include <errno.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>

static int tapfd, sockfd;
static unsigned long send_errors;

static void *down(void *arg) {
  (void)arg;
  static char buf[65600];
  for (;;) {
    ssize_t n = recv(sockfd, buf, sizeof buf, 0);
    if (n < 0) { if (errno == EINTR) continue; perror("recv"); exit(1); }
    if (write(tapfd, buf, (size_t)n) < 0 && errno != EINTR) { /* the guest's kernel drops it */ }
  }
  return NULL;
}

int main(int argc, char **argv) {
  if (argc != 3) { fprintf(stderr, "usage: nicpump TAPFD SOCKFD\n"); return 2; }
  tapfd = atoi(argv[1]); sockfd = atoi(argv[2]);
  pthread_t t;
  pthread_create(&t, NULL, down, NULL);
  static char buf[65600];
  for (;;) {
    ssize_t n = read(tapfd, buf, sizeof buf);
    if (n < 0) { if (errno == EINTR) continue; perror("read tap"); return 1; }
    if (send(sockfd, buf, (size_t)n, 0) < 0) {
      send_errors++;
      if (send_errors <= 3 || send_errors % 100 == 0)
        fprintf(stderr, "send failed (%lu so far): %s\n", send_errors, strerror(errno));
    }
  }
}

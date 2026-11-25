// A stand-in for a member's boundary process: it holds one secret value in memory and waits.
// argv[1] = the secret, argv[2] = "nodump" to mark the process not dumpable (PR_SET_DUMPABLE 0).
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/prctl.h>
int main(int c, char **v) {
  if (c > 2 && !strcmp(v[2], "nodump")) prctl(PR_SET_DUMPABLE, 0);
  char *s = malloc(64); strcpy(s, v[1]); memset(v[1], 0, strlen(v[1]));  // only the heap holds it
  printf("%d\n", getpid()); fflush(stdout); pause(); return 0;
}

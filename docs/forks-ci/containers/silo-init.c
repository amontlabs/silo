// silo-init: the container's first process. Applies a Landlock ruleset, then execs the real command.
//
//   silo-init --probe                       print the kernel's Landlock ABI and exit
//   silo-init [--ro=P:P] [--rw=P:P] [--connect=PORT,PORT] [--bind=PORT,PORT] [--require] -- CMD ARGS...
//
// The ruleset handles every filesystem right of the kernel's ABI (capped at ABI 4) and, on ABI 4, TCP bind and
// connect. Paths in --ro are readable and executable, paths in --rw are fully usable; anything else is denied.
// A missing path is skipped. Without Landlock in the kernel it warns and runs the command unrestricted unless
// --require is given. A kernel ABI below 4 gets a warning: its network rules are not enforced.
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <unistd.h>

#ifndef __NR_landlock_create_ruleset
#define __NR_landlock_create_ruleset 444
#define __NR_landlock_add_rule 445
#define __NR_landlock_restrict_self 446
#endif

#define LL_CREATE_RULESET_VERSION (1U << 0)
#define LL_RULE_PATH_BENEATH 1
#define LL_RULE_NET_PORT 2

#define A_EXECUTE (1ULL << 0)
#define A_WRITE_FILE (1ULL << 1)
#define A_READ_FILE (1ULL << 2)
#define A_READ_DIR (1ULL << 3)
#define A_REMOVE_DIR (1ULL << 4)
#define A_REMOVE_FILE (1ULL << 5)
#define A_MAKE_CHAR (1ULL << 6)
#define A_MAKE_DIR (1ULL << 7)
#define A_MAKE_REG (1ULL << 8)
#define A_MAKE_SOCK (1ULL << 9)
#define A_MAKE_FIFO (1ULL << 10)
#define A_MAKE_BLOCK (1ULL << 11)
#define A_MAKE_SYM (1ULL << 12)
#define A_REFER (1ULL << 13)
#define A_TRUNCATE (1ULL << 14)
#define N_BIND_TCP (1ULL << 0)
#define N_CONNECT_TCP (1ULL << 1)

struct ruleset_attr {
    uint64_t handled_access_fs;
    uint64_t handled_access_net;
};
struct path_beneath_attr {
    uint64_t allowed_access;
    int32_t parent_fd;
} __attribute__((packed));
struct net_port_attr {
    uint64_t allowed_access;
    uint64_t port;
};

static const char *DEFAULT_RO = "/";
static const char *DEFAULT_RW =
    "/tmp:/var:/run:/home:/root:/workspace:/dev:/usr:/etc:/opt:/srv:/mnt";

static int abi_version(void) {
    return (int)syscall(__NR_landlock_create_ruleset, NULL, 0, LL_CREATE_RULESET_VERSION);
}

static int add_path(int ruleset, const char *path, uint64_t allowed, uint64_t handled) {
    int fd = open(path, O_PATH | O_CLOEXEC);
    if (fd < 0) {
        if (errno == ENOENT || errno == EACCES) return 0;
        fprintf(stderr, "silo-init: open %s: %s\n", path, strerror(errno));
        return -1;
    }
    struct stat st;
    if (fstat(fd, &st) < 0) { close(fd); return -1; }
    allowed &= handled;
    if (!S_ISDIR(st.st_mode)) allowed &= (A_EXECUTE | A_WRITE_FILE | A_READ_FILE | A_TRUNCATE);
    struct path_beneath_attr attr = { .allowed_access = allowed, .parent_fd = fd };
    int rc = (int)syscall(__NR_landlock_add_rule, ruleset, LL_RULE_PATH_BENEATH, &attr, 0);
    if (rc < 0) fprintf(stderr, "silo-init: add_rule %s: %s\n", path, strerror(errno));
    close(fd);
    return rc;
}

static int add_ports(int ruleset, const char *list, uint64_t access) {
    if (!list) return 0;
    char *copy = strdup(list), *save = NULL;
    for (char *tok = strtok_r(copy, ",", &save); tok; tok = strtok_r(NULL, ",", &save)) {
        struct net_port_attr attr = { .allowed_access = access, .port = (uint64_t)atoi(tok) };
        if (syscall(__NR_landlock_add_rule, ruleset, LL_RULE_NET_PORT, &attr, 0) < 0) {
            fprintf(stderr, "silo-init: add_rule port %s: %s\n", tok, strerror(errno));
            free(copy);
            return -1;
        }
    }
    free(copy);
    return 0;
}

static int add_paths(int ruleset, const char *list, uint64_t allowed, uint64_t handled) {
    char *copy = strdup(list), *save = NULL;
    for (char *tok = strtok_r(copy, ":", &save); tok; tok = strtok_r(NULL, ":", &save))
        if (add_path(ruleset, tok, allowed, handled) < 0) { free(copy); return -1; }
    free(copy);
    return 0;
}

int main(int argc, char **argv) {
    const char *ro = DEFAULT_RO, *rw = DEFAULT_RW, *connect_ports = NULL, *bind_ports = NULL;
    int require = 0, i = 1;
    if (argc == 2 && !strcmp(argv[1], "--probe")) {
        int abi = abi_version();
        if (abi < 0) printf("landlock unavailable: %s\n", strerror(errno));
        else printf("landlock abi %d\n", abi);
        return abi < 0 ? 1 : 0;
    }
    for (; i < argc; i++) {
        if (!strcmp(argv[i], "--")) { i++; break; }
        else if (!strncmp(argv[i], "--ro=", 5)) ro = argv[i] + 5;
        else if (!strncmp(argv[i], "--rw=", 5)) rw = argv[i] + 5;
        else if (!strncmp(argv[i], "--connect=", 10)) connect_ports = argv[i] + 10;
        else if (!strncmp(argv[i], "--bind=", 7)) bind_ports = argv[i] + 7;
        else if (!strcmp(argv[i], "--require")) require = 1;
        else { fprintf(stderr, "silo-init: unknown option %s\n", argv[i]); return 2; }
    }
    if (i >= argc) { fprintf(stderr, "silo-init: no command\n"); return 2; }

    if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) < 0) {
        fprintf(stderr, "silo-init: no_new_privs: %s\n", strerror(errno));
        return 1;
    }
    int abi = abi_version();
    if (abi < 0) {
        fprintf(stderr, "silo-init: WARNING: Landlock is not available (%s); running without it\n", strerror(errno));
        if (require) return 1;
        execvp(argv[i], argv + i);
        fprintf(stderr, "silo-init: exec %s: %s\n", argv[i], strerror(errno));
        return 127;
    }
    if (abi < 4)
        fprintf(stderr, "silo-init: WARNING: Landlock ABI %d is below 4; TCP network rules are not enforced\n", abi);

    uint64_t fs = A_EXECUTE | A_WRITE_FILE | A_READ_FILE | A_READ_DIR | A_REMOVE_DIR | A_REMOVE_FILE |
                  A_MAKE_CHAR | A_MAKE_DIR | A_MAKE_REG | A_MAKE_SOCK | A_MAKE_FIFO | A_MAKE_BLOCK | A_MAKE_SYM;
    if (abi >= 2) fs |= A_REFER;
    if (abi >= 3) fs |= A_TRUNCATE;
    uint64_t net = abi >= 4 ? (N_BIND_TCP | N_CONNECT_TCP) : 0;
    struct ruleset_attr attr = { .handled_access_fs = fs, .handled_access_net = net };
    size_t size = abi >= 4 ? sizeof(attr) : sizeof(uint64_t);
    int ruleset = (int)syscall(__NR_landlock_create_ruleset, &attr, size, 0);
    if (ruleset < 0) {
        fprintf(stderr, "silo-init: create_ruleset: %s\n", strerror(errno));
        return 1;
    }
    uint64_t read_exec = A_EXECUTE | A_READ_FILE | A_READ_DIR;
    if (add_paths(ruleset, ro, read_exec, fs) < 0 || add_paths(ruleset, rw, fs, fs) < 0 ||
        (net && (add_ports(ruleset, connect_ports, N_CONNECT_TCP) < 0 || add_ports(ruleset, bind_ports, N_BIND_TCP) < 0)))
        return 1;
    if (syscall(__NR_landlock_restrict_self, ruleset, 0) < 0) {
        fprintf(stderr, "silo-init: restrict_self: %s\n", strerror(errno));
        return 1;
    }
    close(ruleset);
    fprintf(stderr, "silo-init: landlock abi=%d fs=0x%llx net=0x%llx enforced\n", abi,
            (unsigned long long)fs, (unsigned long long)net);
    execvp(argv[i], argv + i);
    fprintf(stderr, "silo-init: exec %s: %s\n", argv[i], strerror(errno));
    return 127;
}

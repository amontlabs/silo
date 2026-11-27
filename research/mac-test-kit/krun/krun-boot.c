/* krun-boot: boots one Linux computer through libkrun's C interface, as Silo's helper would.
 * A copy of #381's research/2026-11-25-linux-main/kit/krun-boot.c for the Mac kit's tests 47 to 49,
 * with two additions: --initrd (Ubuntu's own kernel needs its initrd; #381 builds Silo's kernel,
 * which a Mac cannot build), and a stop request: SIGTERM writes to libkrun's shutdown eventfd
 * (krun_get_shutdown_eventfd), which asks the guest to power off; whether it does is test 47's
 * question (#381 L1). ./kit build compiles it when Homebrew's libkrun is installed and signs it
 * with krun/krun.entitlements (com.apple.security.hypervisor).
 * Written against libkrun 1.19 (include/libkrun.h on stable-1.19.x). The kit's tests start it;
 * Silo's helper is Zig on libkrun 2.0 (2-main-runs.md). Build: cc -O2 -o krun-boot krun-boot.c -lkrun
 *
 * krun-boot --kernel IMAGE --disk DISK.raw [--seed SEED.iso] [--cpus N] [--mem MiB]
 *           --ssh-sock PATH [--probe-sock PATH] [--net-sock PATH] [--console PATH]
 *           [--cmdline STR] [--kernel-format raw|elf] [--initrd FILE]
 * Without --net-sock the computer has no network device: SSH still works over vsock. */
#include <libkrun.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>
#include <signal.h>
#include <unistd.h>

static int shutdown_fd = -1;
static void on_term(int sig) {
	(void)sig;
	uint64_t one = 1;
	if (shutdown_fd < 0 || write(shutdown_fd, &one, sizeof one) != sizeof one) _exit(143);
}

static void die(const char *what, int32_t err) {
	fprintf(stderr, "krun-boot: %s failed: %d\n", what, err);
	exit(1);
}

int main(int argc, char **argv) {
	const char *kernel = 0, *disk = 0, *seed = 0, *ssh_sock = 0, *probe_sock = 0;
	const char *net_sock = 0, *console = 0, *initrd = 0;
	const char *cmdline = "root=/dev/vda1 rootfstype=ext4 rw console=hvc0 init=/lib/systemd/systemd";
	int cpus = 4, mem = 4096;
	uint32_t kformat = KRUN_KERNEL_FORMAT_RAW;   /* an arm64 Image; x86 hosts pass --kernel-format elf */
	for (int i = 1; i + 1 < argc; i += 2) {
		const char *k = argv[i], *v = argv[i + 1];
		if (!strcmp(k, "--kernel")) kernel = v;
		else if (!strcmp(k, "--disk")) disk = v;
		else if (!strcmp(k, "--seed")) seed = v;
		else if (!strcmp(k, "--ssh-sock")) ssh_sock = v;
		else if (!strcmp(k, "--probe-sock")) probe_sock = v;
		else if (!strcmp(k, "--net-sock")) net_sock = v;
		else if (!strcmp(k, "--console")) console = v;
		else if (!strcmp(k, "--cmdline")) cmdline = v;
		else if (!strcmp(k, "--initrd")) initrd = v;
		else if (!strcmp(k, "--kernel-format")) kformat = !strcmp(v, "elf") ? KRUN_KERNEL_FORMAT_ELF : KRUN_KERNEL_FORMAT_RAW;
		else if (!strcmp(k, "--cpus")) cpus = atoi(v);
		else if (!strcmp(k, "--mem")) mem = atoi(v);
		else { fprintf(stderr, "krun-boot: unknown %s\n", k); return 2; }
	}
	if (!kernel || !disk || !ssh_sock) { fprintf(stderr, "krun-boot: --kernel, --disk and --ssh-sock are needed\n"); return 2; }

	int32_t ctx = krun_create_ctx(), e;
	if (ctx < 0) die("krun_create_ctx", ctx);
	if ((e = krun_set_vm_config(ctx, (uint8_t)cpus, (uint32_t)mem))) die("krun_set_vm_config", e);
	/* In 1.19, KRUN_KERNEL_FORMAT_RAW is 0; in 2.0 the constants are renumbered (ELF is 0). */
	if ((e = krun_set_kernel(ctx, kernel, kformat, initrd, cmdline))) die("krun_set_kernel", e);
	if ((e = krun_disable_implicit_init(ctx))) die("krun_disable_implicit_init", e);
	if ((e = krun_add_disk2(ctx, "root", disk, KRUN_DISK_FORMAT_RAW, false))) die("krun_add_disk2 root", e);
	if (seed && (e = krun_add_disk2(ctx, "seed", seed, KRUN_DISK_FORMAT_RAW, true))) die("krun_add_disk2 seed", e);
	/* vsock with no TSI: nothing of the network is hijacked; only the ports below exist. */
	if ((e = krun_disable_implicit_vsock(ctx))) die("krun_disable_implicit_vsock", e);
	if ((e = krun_add_vsock(ctx, 0))) die("krun_add_vsock", e);
	if ((e = krun_add_vsock_port2(ctx, 22, ssh_sock, true))) die("krun_add_vsock_port2 22", e);
	if (probe_sock && (e = krun_add_vsock_port2(ctx, 1024, probe_sock, false))) die("krun_add_vsock_port2 1024", e);
	if (net_sock) {
		uint8_t mac[6] = {0x5a, 0x94, 0xef, 0xe4, 0x0c, 0x01};
		if ((e = krun_add_net_unixgram(ctx, net_sock, -1, mac, COMPAT_NET_FEATURES, NET_FLAG_VFKIT)))
			die("krun_add_net_unixgram", e);
	}
	if (console && (e = krun_set_console_output(ctx, console))) die("krun_set_console_output", e);
	shutdown_fd = krun_get_shutdown_eventfd(ctx);
	fprintf(stderr, "krun-boot: shutdown eventfd %d\n", shutdown_fd);
	signal(SIGTERM, on_term);
	/* Takes this thread and never returns while the computer runs; the process exits with it. */
	e = krun_start_enter(ctx);
	die("krun_start_enter", e);
	return 1;
}

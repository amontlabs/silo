/* plugwriter: a record writer whose disk loses power, for test 45 (#386: what a power cut keeps
 * depends on F_FULLFSYNC).
 *   plugwriter write FILE MODE    appends numbered 512-byte lines, each with a CRC, and after each
 *                                 one forces it out with MODE: none, fsync, barrier (F_BARRIERFSYNC)
 *                                 or full (F_FULLFSYNC; fdatasync on Linux). After each forced write
 *                                 it prints "ack N" on stdout, unbuffered: the line the writer
 *                                 would tell its source is safe. Runs until a write fails or it is
 *                                 killed.
 *   plugwriter check FILE LASTACK prints one JSON line: the lines found whole from the start, the
 *                                 first damaged line, whole lines after a damaged one (damage in
 *                                 the middle, not only an unfinished end), and how many lines that
 *                                 were acknowledged are missing or damaged. */
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define LINE 512

static uint32_t crc32(const unsigned char *p, size_t n) {
	uint32_t c = 0xffffffffu;
	for (size_t i = 0; i < n; i++) {
		c ^= p[i];
		for (int k = 0; k < 8; k++) c = (c >> 1) ^ (0xedb88320u & (0u - (c & 1)));
	}
	return ~c;
}

static void make_line(unsigned char *b, unsigned long long n) {
	memset(b, 'x', LINE);
	int h = snprintf((char *)b, LINE, "SILO %012llu ", n);
	b[h] = 'x';
	uint32_t c = crc32(b, LINE - 10);
	snprintf((char *)b + LINE - 10, 10, "%08x\n", c);
	b[LINE - 1] = '\n';
}

static int line_ok(const unsigned char *b, unsigned long long n) {
	char want[32];
	snprintf(want, sizeof want, "SILO %012llu ", n);
	if (memcmp(b, want, strlen(want))) return 0;
	char crc[9];
	snprintf(crc, sizeof crc, "%08x", crc32(b, LINE - 10));
	return !memcmp(b + LINE - 10, crc, 8) && b[LINE - 1] == '\n';
}

static int force(int fd, const char *mode) {
	if (!strcmp(mode, "none")) return 0;
	if (!strcmp(mode, "fsync")) return fsync(fd);
#ifdef __APPLE__
	if (!strcmp(mode, "barrier")) return fcntl(fd, F_BARRIERFSYNC);
	if (!strcmp(mode, "full")) return fcntl(fd, F_FULLFSYNC);
#else
	if (!strcmp(mode, "barrier") || !strcmp(mode, "full")) return fdatasync(fd);
#endif
	errno = EINVAL;
	return -1;
}

int main(int argc, char **argv) {
	if (argc == 4 && !strcmp(argv[1], "write")) {
		int fd = open(argv[2], O_WRONLY | O_CREAT | O_TRUNC, 0644);
		if (fd < 0) { perror("open"); return 1; }
		setvbuf(stdout, NULL, _IONBF, 0);
		unsigned char b[LINE];
		for (unsigned long long n = 0;; n++) {
			make_line(b, n);
			if (write(fd, b, LINE) != LINE) { fprintf(stderr, "write %llu: %s\n", n, strerror(errno)); return 2; }
			if (force(fd, argv[3])) { fprintf(stderr, "force %llu: %s\n", n, strerror(errno)); return 2; }
			printf("ack %llu\n", n);
		}
	}
	if (argc == 4 && !strcmp(argv[1], "check")) {
		long long last_ack = atoll(argv[3]);
		FILE *f = fopen(argv[2], "rb");
		struct stat st;
		if (!f || fstat(fileno(f), &st)) { printf("{\"error\":\"cannot read %s\"}\n", argv[2]); return 1; }
		unsigned long long lines = st.st_size / LINE, prefix = 0, after_damage = 0, lost = 0;
		long long first_bad = -1;
		unsigned char b[LINE];
		for (unsigned long long n = 0; n < lines; n++) {
			if (fread(b, 1, LINE, f) != LINE) break;
			int ok = line_ok(b, n);
			if (ok && first_bad < 0) prefix = n + 1;
			if (!ok && first_bad < 0) first_bad = (long long)n;
			if (ok && first_bad >= 0) after_damage++;
			if (!ok && (long long)n <= last_ack) lost++;
		}
		if (last_ack >= 0 && (long long)lines <= last_ack) lost += (unsigned long long)(last_ack + 1) - lines;
		printf("{\"bytes\":%lld,\"lines\":%llu,\"whole_prefix\":%llu,\"first_damaged\":%lld,\"whole_after_damage\":%llu,"
		       "\"last_ack\":%lld,\"acked_lost\":%llu}\n",
		       (long long)st.st_size, lines, prefix, first_bad, after_damage, last_ack, lost);
		return 0;
	}
	fprintf(stderr, "usage: plugwriter write FILE none|fsync|barrier|full\n       plugwriter check FILE LASTACK\n");
	return 2;
}

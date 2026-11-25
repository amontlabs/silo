/* cloneattrs: what APFS says a file holds of its own, for test 46.
 *   cloneattrs FILE...    one JSON line per file: clone id, private size, clone reference count,
 *                         allocated size, and the volume's free bytes (statfs).
 * #378: the private size APFS reports (ATTR_CMNEXT_PRIVATESIZE) reads 0 for older files while a
 * local Time Machine snapshot exists; files of one clone id (ATTR_CMNEXT_CLONEID) share blocks.
 * On Linux these attributes do not exist: it prints the allocated size and free bytes only, with
 * "apfs": false, so the test's logic runs. */
#include <stdio.h>
#include <stdint.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#if defined(__APPLE__)
#include <sys/attr.h>
#include <sys/mount.h>
#include <unistd.h>
#else
#include <sys/statvfs.h>
#endif

static unsigned long long free_bytes(const char *p) {
#if defined(__APPLE__)
	struct statfs s;
	if (statfs(p, &s)) return 0;
	return (unsigned long long)s.f_bavail * s.f_bsize;
#else
	struct statvfs s;
	if (statvfs(p, &s)) return 0;
	return (unsigned long long)s.f_bavail * s.f_frsize;
#endif
}

int main(int argc, char **argv) {
	int bad = 0;
	for (int i = 1; i < argc; i++) {
		struct stat st;
		if (stat(argv[i], &st)) { printf("{\"path\":\"%s\",\"error\":\"stat\"}\n", argv[i]); bad = 1; continue; }
		unsigned long long alloc = (unsigned long long)st.st_blocks * 512, freeb = free_bytes(argv[i]);
#if defined(__APPLE__) && defined(ATTR_CMNEXT_PRIVATESIZE) && defined(ATTR_CMNEXT_CLONEID)
		struct attrlist al;
		memset(&al, 0, sizeof al);
		al.bitmapcount = ATTR_BIT_MAP_COUNT;
		al.commonattr = ATTR_CMN_RETURNED_ATTRS;
		al.forkattr = ATTR_CMNEXT_PRIVATESIZE | ATTR_CMNEXT_CLONEID
#ifdef ATTR_CMNEXT_CLONE_REFCNT
			| ATTR_CMNEXT_CLONE_REFCNT
#endif
			;
		struct __attribute__((packed)) {
			uint32_t len;
			attribute_set_t returned;
			off_t private_size;
			uint64_t clone_id;
#ifdef ATTR_CMNEXT_CLONE_REFCNT
			uint32_t refcnt;
#endif
		} buf;
		memset(&buf, 0, sizeof buf);
		if (getattrlist(argv[i], &al, &buf, sizeof buf, FSOPT_ATTR_CMN_EXTENDED | FSOPT_NOFOLLOW)) {
			printf("{\"path\":\"%s\",\"apfs\":false,\"allocated\":%llu,\"volume_free\":%llu}\n", argv[i], alloc, freeb);
			continue;
		}
		long long refcnt = -1;
#ifdef ATTR_CMNEXT_CLONE_REFCNT
		if (buf.returned.forkattr & ATTR_CMNEXT_CLONE_REFCNT) refcnt = buf.refcnt;
#endif
		printf("{\"path\":\"%s\",\"apfs\":true,\"private_size\":%lld,\"clone_id\":%llu,\"clone_refcnt\":%lld,"
		       "\"allocated\":%llu,\"volume_free\":%llu}\n",
		       argv[i], (long long)buf.private_size, (unsigned long long)buf.clone_id, refcnt, alloc, freeb);
#else
		printf("{\"path\":\"%s\",\"apfs\":false,\"allocated\":%llu,\"volume_free\":%llu}\n", argv[i], alloc, freeb);
#endif
	}
	return bad;
}

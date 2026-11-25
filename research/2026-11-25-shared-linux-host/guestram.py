# Resident memory of a QEMU process's guest RAM: the RSS of its largest anonymous mapping,
# read from /proc/<pid>/smaps, so that QEMU's own memory (and TCG's code cache) is left out.
# argv[1:] = pids. Prints one line per pid, in MiB.
import sys
for pid in sys.argv[1:]:
    best = (0, 0)
    cur = None
    for line in open(f"/proc/{pid}/smaps"):
        f = line.split()
        if "-" in f[0] and len(f) >= 5:
            lo, hi = (int(x, 16) for x in f[0].split("-")); cur = hi - lo
        elif f[0] == "Rss:" and cur and cur > best[0]:
            best = (cur, int(f[1]))
    print(pid, best[0] >> 20, "MiB mapped,", best[1] >> 10, "MiB resident")

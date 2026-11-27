# Another process on the host looks for a secret in a process's memory, through /proc/<pid>/mem.
# argv[1] = pid, argv[2] = the secret's prefix. Prints "read <value>" or why it could not.
import re, sys
pid, prefix = sys.argv[1], sys.argv[2].encode()
try:
    maps = open(f"/proc/{pid}/maps").read().splitlines(); mem = open(f"/proc/{pid}/mem", "rb", 0)
except OSError as e:
    print("refused:", e.strerror); sys.exit()
for m in maps:
    a, perms = m.split()[0], m.split()[1]
    if "r" not in perms: continue
    lo, hi = (int(x, 16) for x in a.split("-"))
    try: mem.seek(lo); data = mem.read(hi - lo)
    except OSError: continue
    i = data.find(prefix)
    if i >= 0: print("read", data[i:i+40].split(b"\0")[0].decode()); sys.exit()
print("not found")

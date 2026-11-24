"""Where are the commit objects inside a push pack? Prints, per pack, the index of the first
non-commit object and the byte offset after the last commit. Usage: packorder.py <repo> <new> <old>"""
import subprocess, sys, zlib
repo, new, old = sys.argv[1:4]
pack = subprocess.run(["git", "-C", repo, "pack-objects", "--revs", "--thin", "--stdout", "--delta-base-offset", "-q"],
                      input=f"{new}\n^{old}\n".encode(), capture_output=True, check=True).stdout
n = int.from_bytes(pack[8:12], "big"); pos = 12; types = []; last_commit_end = 0
for k in range(n):
    c = pack[pos]; pos += 1; t = (c >> 4) & 7
    while c & 0x80:
        c = pack[pos]; pos += 1
    if t == 6:
        c = pack[pos]; pos += 1
        while c & 0x80:
            c = pack[pos]; pos += 1
    elif t == 7:
        pos += 20
    d = zlib.decompressobj(); rest = pack[pos:pos + 1 << 30]
    d.decompress(pack[pos:]); pos = len(pack) - len(d.unused_data)
    types.append(t)
    if t == 1:
        last_commit_end = pos
first_non_commit = next((i for i, t in enumerate(types) if t != 1), n)
print({"objects": n, "commits": types.count(1), "first_non_commit_index": first_non_commit,
       "bytes_until_last_commit": last_commit_end, "pack_bytes": len(pack),
       "share": round(last_commit_end / len(pack), 4)})

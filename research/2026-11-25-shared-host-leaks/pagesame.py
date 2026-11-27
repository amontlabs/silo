# Compares guest memory dumps page by page with the first one, the snapshot.
# A fork that maps the snapshot copy-on-write (MAP_PRIVATE) keeps a page shared until the guest
# writes it; a page with the same content at the same place is the upper bound of that.
# argv[1] = snapshot dump; argv[2:] = later dumps. Prints, per dump, the share of pages unchanged.
import sys, hashlib
PG = 4096
def hashes(p):
    out = []
    with open(p, "rb") as f:
        while (b := f.read(PG)):
            out.append(hashlib.blake2b(b, digest_size=8).digest())
    return out
s = hashes(sys.argv[1]); zero = hashlib.blake2b(bytes(PG), digest_size=8).digest()
print(f"snapshot: {len(s)} pages, {sum(h == zero for h in s)} of them zeros")
for p in sys.argv[2:]:
    d = hashes(p); same = sum(a == b for a, b in zip(s, d)); samez = sum(a == b == zero for a, b in zip(s, d))
    print(f"{p.split('/')[-1]}: {same} of {len(s)} pages unchanged ({100*same/len(s):.1f}%; {(same-samez)*4//1024} MiB of it not zeros)")

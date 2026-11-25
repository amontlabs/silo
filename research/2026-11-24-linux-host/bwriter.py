"""Neighbour writer: 4 KiB O_DIRECT writes at random places of its own image, fdatasync every 8,
until the file <stop> exists. Writes (start time, duration) of each operation to <out>."""
import mmap, os, random, sys, time, json
img, stop, out = sys.argv[1], sys.argv[2], sys.argv[3]
BS = 4096; n = os.path.getsize(img) // BS
buf = mmap.mmap(-1, BS); buf.write(os.urandom(BS))
fd = os.open(img, os.O_WRONLY | os.O_DIRECT)
lat = []; i = 0
while i % 256 or not os.path.exists(stop):
    t = time.time()
    os.pwritev(fd, [buf], random.randrange(n) * BS)
    i += 1
    if i % 8 == 0: os.fdatasync(fd)
    lat.append((t, time.time() - t))
os.close(fd)
json.dump(lat, open(out, "w"))

"""#381 proto/ext4read.py against the Linux kernel's own ext4, on images a computer could leave
behind. Each case changes a clean image with debugfs or by hand, mounts it read-only through a
loop device (the kernel's reading), walks it with ext4read.py (Silo's reading), and compares the
paths and types. usage: python3 differ.py <ext4read.py> (root, loop devices)"""
import os, subprocess, sys, struct, shutil, json
R = sys.argv[1]; W = "/tmp/claude-0/ext4d"; MNT = W + "/mnt"
os.makedirs(MNT, exist_ok=True)
sh = lambda c: subprocess.run(c, shell=True, capture_output=True, text=True)

def mkimg(name, feats="^has_journal,^metadata_csum"):
    src = W + "/src"; shutil.rmtree(src, ignore_errors=True)
    os.makedirs(src + "/c"); os.makedirs(src + "/a")
    for i in range(1, 41): open(f"{src}/c/file_with_a_long_name_{i:02d}", "w").write(str(i))
    open(src + "/a/f1", "w").write("hello")
    img = f"{W}/{name}.img"
    if os.path.exists(img): os.remove(img)
    sh(f"truncate -s 4M {img} && mkfs.ext4 -q -b 1024 -O {feats} -d {src} {img}")
    return img

def silo(img):
    r = sh(f"python3 {R} walk {img}")
    if r.returncode != 0:
        return None, (r.stderr.strip().splitlines() or ["?"])[-1]
    out = {}
    for l in r.stdout.splitlines():
        if l.startswith("#"): continue
        f = l.split("\t")
        if not f[-1].startswith("/lost+found"): out[f[-1]] = f[2]
    return out, None

def kernel(img):
    sh(f"umount {MNT}")
    r = sh(f"mount -o loop,ro,noload {img} {MNT}")
    if r.returncode != 0: return None, "mount: " + r.stderr.strip()
    r = sh(f"cd {MNT} && find . -mindepth 1 -not -path './lost+found*' -printf '%y %p\\n'")
    errs = [l for l in r.stderr.splitlines() if l]
    out = {}
    for l in r.stdout.splitlines():
        t, p = l.split(" ", 1); out[p[1:]] = {"d": "dir", "f": "file", "l": "link"}.get(t, "other")
    sh(f"umount {MNT}")
    return out, ("; ".join(errs)[:200] or None)

def inode_offset(img, ino):
    r = sh(f"debugfs -R 'imap <{ino}>' {img}")
    # "Inode 12 is part of block group 0\n\tlocated at block 37, offset 0x0b00"
    blk = int(r.stdout.split("located at block ")[1].split(",")[0]); off = int(r.stdout.split("offset ")[1].strip(), 16)
    return blk * 1024 + off

def ino_of(img, path):
    return int(sh(f"debugfs -R 'stat {path}' {img}").stdout.split("Inode: ")[1].split()[0])

def patch(img, off, b):
    with open(img, "r+b") as f: f.seek(off); f.write(b)

CASES = []
def case(f): CASES.append(f); return f

@case
def clean():
    """No change: both readings must agree."""
    return mkimg("clean")

@case
def dir_size_below_blocks():
    """/c has two blocks; its size is set to one. The kernel reads a directory up to its size."""
    img = mkimg("dirsize"); sh(f"debugfs -w -R 'sif /c size 1024' {img}"); return img

@case
def dir_extent_unwritten():
    """/c's extent is marked unwritten. The kernel reads an unwritten extent as zeroes."""
    img = mkimg("unwritten"); o = inode_offset(img, ino_of(img, "/c")) + 40 + 12 + 4
    ln = struct.unpack("<H", open(img, "rb").read()[o:o + 2])[0]; patch(img, o, struct.pack("<H", ln + 32768)); return img

@case
def deleted_inode_linked():
    """/a/f1's inode is marked deleted (links 0, dtime set) and still named in /a."""
    img = mkimg("deleted"); sh(f"debugfs -w -R 'sif /a/f1 links_count 0' {img}"); sh(f"debugfs -w -R 'sif /a/f1 dtime 1' {img}"); return img

@case
def dirent_name_with_nul():
    """A name in /c carries a NUL byte, which no path may hold."""
    img = mkimg("nul"); data = open(img, "rb").read(); o = data.find(b"file_with_a_long_name_07"); patch(img, o + 4, b"\0"); return img

@case
def csum_dir_block_changed():
    """metadata_csum on; a name in /c is changed without its block's checksum."""
    img = mkimg("csum", "^has_journal,metadata_csum"); data = open(img, "rb").read(); o = data.find(b"file_with_a_long_name_07"); patch(img, o, b"X"); return img

@case
def dirent_type_0xde():
    """A directory entry's type byte is 0xDE, the value the checksum tail uses, on a file system
    without metadata_csum. The kernel lists it with an unknown type."""
    img = mkimg("de"); data = open(img, "rb").read(); o = data.find(b"file_with_a_long_name_07"); patch(img, o - 1, b"\xde"); return img

rows = []
for c in CASES:
    img = c()
    s, serr = silo(img); k, kerr = kernel(img)
    if s is None or k is None:
        verdict = "silo refuses" if s is None else "kernel refuses"
        diff = serr if s is None else kerr
    else:
        only_s = sorted(set(s) - set(k)); only_k = sorted(set(k) - set(s))
        types = sorted(p for p in set(s) & set(k) if s[p] != k[p])
        verdict = "same" if not (only_s or only_k or types) else "DIFFERENT"
        diff = {"only_silo": only_s[:5], "n_only_silo": len(only_s), "only_kernel": only_k[:5], "n_only_kernel": len(only_k),
                "type": [(p, s[p], k[p]) for p in types][:3], "kernel_errors": kerr}
    rows.append({"case": c.__name__, "verdict": verdict, "detail": diff})
    print(f"{c.__name__:26} {verdict:15} {json.dumps(diff)[:240]}")
json.dump(rows, open(os.environ.get("JSON", W + "/differ.json"), "w"), indent=1)

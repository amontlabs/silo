"""For tests 47 to 49: an arm64 Linux kernel as libkrun's KRUN_KERNEL_FORMAT_RAW wants it.

    python3 -I py/linuxmain/kernel.py IN OUT

IN is Ubuntu's vmlinuz: an EFI zboot image (a PE file whose header says "zimg" and where its
gzip or zstd payload is), a gzip file, or already a raw Image. OUT is the raw Image, checked by
its arm64 magic ("ARM\\x64" at offset 56). Prints which form IN was.
"""
import gzip
import struct
import sys


def decompress(kind, data):
    if kind == "gzip":
        return gzip.decompress(data)
    if kind == "zstd":
        try:
            from compression import zstd  # Python 3.14
        except ImportError:
            sys.exit("a zstd zboot kernel needs Python 3.14 or later")
        return zstd.decompress(data)
    sys.exit("unknown zboot compression %r" % kind)


def main(src, dst):
    data = open(src, "rb").read()
    if data[:2] == b"MZ" and data[4:8] == b"zimg":
        off, size = struct.unpack("<II", data[8:16])
        kind = data[24:32].rstrip(b"\0").decode()
        img, form = decompress(kind, data[off:off + size]), "zboot " + kind
    elif data[:2] == b"\x1f\x8b":
        img, form = gzip.decompress(data), "gzip"
    else:
        img, form = data, "raw"
    if img[56:60] != b"ARM\x64":
        sys.exit("%s: not an arm64 Image after %s" % (src, form))
    open(dst, "wb").write(img)
    print(form)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])

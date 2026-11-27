"""Coverage-guided fuzzing of Apple's open-source lzfse decoder, on input root in main controls: a
decmpfs block of LZFSE, or of LZVN wrapped as #390's probe wraps it.

The library is built with AddressSanitizer and libFuzzer's coverage (see run-fuzz.sh), and loaded
into atheris, whose libFuzzer drives it. A read or write out of bounds stops the run with
AddressSanitizer's report.

  LD_PRELOAD=$(gcc -print-file-name=libasan.so) python3 fuzz_lzfse.py LIB CORPUS_DIR [libFuzzer flags]
"""
import ctypes
import struct
import sys

import atheris

# libFuzzer's coverage callbacks live in atheris's own library: make them global before the
# instrumented decoder is loaded, so that its counters register with the fuzzer.
for name in ("core_with_libfuzzer",):
    import glob, os
    for so in glob.glob(os.path.join(os.path.dirname(atheris.__file__), name + "*.so")):
        ctypes.CDLL(so, mode=ctypes.RTLD_GLOBAL)
lib = ctypes.CDLL(sys.argv[1])
dec = lib.lzfse_decode_buffer
dec.restype = ctypes.c_size_t
dec.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p]
scratch = ctypes.create_string_buffer(lib.lzfse_decode_scratch_size() + 1)


def one(data):
    if len(data) < 3:
        return
    # First byte: the format; next two: the block's size as decmpfs's header would give it.
    fmt, size = data[0] & 1, struct.unpack_from("<H", data, 1)[0] + 1
    body = bytes(data[3:])
    if fmt == 1:
        body = b"bvxn" + struct.pack("<II", size, len(body)) + body + b"bvx$"
    # The exact size the probe asks for: a buffer of the block's size plus 64, as it allocates.
    src = ctypes.create_string_buffer(body, len(body))  # exact, so a read past the end is seen
    dst = ctypes.create_string_buffer(size + 64)
    dec(dst, size + 64, src, len(body), scratch)


atheris.Setup([sys.argv[0]] + sys.argv[2:], one)
atheris.Fuzz()

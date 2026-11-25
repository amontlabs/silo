# A stand-in for a computer's VM process: takes argv[1] MiB of memory, 64 MiB at a time, and holds it.
import sys, time
blocks = []
for _ in range(int(sys.argv[1]) // 64):
    blocks.append(bytearray(64 << 20)); time.sleep(float(sys.argv[2]) if len(sys.argv) > 2 else 0.05)
time.sleep(3600)

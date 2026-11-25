"""Bytes a Mac fetches: the index every day, and a whole table each time it changes.
A whole table, never a difference: a difference names the version a Mac has.

usage: sizes.py <table.json> > results/sizes.txt"""
import gzip, json, sys, datetime, hashlib
sys.path.insert(0, __import__("os").path.join(__import__("os").path.dirname(__file__), "..", "channel"))
import publish
data = open(sys.argv[1], "rb").read()
compact = json.dumps(json.loads(data), separators=(",", ":")).encode()
idx = publish.make_index(1, data, "0.3.0", datetime.datetime(2026, 11, 25, tzinfo=datetime.UTC))
print(f"table, as #396 wrote it: {len(data)} bytes, gzip {len(gzip.compress(data, 9))}")
print(f"table, compact JSON:     {len(compact)} bytes, gzip {len(gzip.compress(compact, 9))}")
print(f"index:                   {len(idx)} bytes; signature file 99 bytes")
rel = 1.7 * 52 + 8 + 7  # a release for each new model week and each price change or fix in 2026 (#396's counts)
g = len(gzip.compress(data, 9))
print(f"a year: 365 indexes ({365 * (len(idx) + 99) // 1024} KiB) and at most {rel:.0f} tables ({rel * g / 1024 / 1024:.1f} MiB gzip), "
      f"fewer when changes share a release")

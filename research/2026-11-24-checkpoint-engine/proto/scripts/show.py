#!/usr/bin/env python3
"""show.py results/*.jsonl: one table line per checkpoint and per drop."""
import json, sys
for f in sys.argv[1:]:
    print("==", f)
    for l in open(f):
        d = json.loads(l); ph = d.pop("phase")
        if ph == "ckpt":
            print(f"ckpt {d['n']}: op {d['op_s']:.3f}s pause {d['pause_ms']:.3f}ms longest {d['longest_write_ms']:.1f}ms "
                  f"(clone {d['in_clone_ms']:.1f} / after {d['after_clone_ms']:.1f} / publish {d['publish_ms']:.1f}) "
                  f"copied {d['copied_mib']}MiB cow {d['cow_copies']} ranges {d['ranges']}")
        elif ph == "verify":
            continue
        else:
            print(ph, d)
    v = [json.loads(l) for l in open(f) if '"verify"' in l]
    if v: print("verify:", sum(1 for x in v if x.get("ok")), "of", len(v), "one moment, in order")

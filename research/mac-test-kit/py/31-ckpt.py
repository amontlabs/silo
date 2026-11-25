"""Reads the runs of test 31 (the checkpoint engine's prototype, `ckpt bench`, `ckpt crash`) and
prints what the test records, one tab-separated line each:

  M <name> <value> <unit> <context>     a measure
  F <name> <value>                      a fact
  O <key> <sentence>                    the outcome (last line)

  31-ckpt.py DIR
      DIR holds <way>.jsonl for the ways run (full, cbw, nbd, delta: each line one phase of
      `ckpt bench`), and optionally crash.jsonl (`ckpt crash`), mutants.txt ("<name> caught" or
      "<name> NOT caught" per line) and pieces.txt ("<way> <pieces>" per line).
"""
import json
import os
import sys

WAYS = ["full", "cbw", "nbd", "delta"]


def load(path):
    out = []
    for line in open(path):
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            out.append(json.loads(line))
        except ValueError:
            pass
    return out


def main(d):
    lines = []
    M = lambda n, v, u, c="": lines.append("M\t%s\t%s\t%s\t%s" % (n, ("%.3f" % v) if isinstance(v, float) else v, u, c))
    F = lambda n, v: lines.append("F\t%s\t%s" % (n, v))
    got = {}
    for w in WAYS:
        p = os.path.join(d, w + ".jsonl")
        if os.path.exists(p):
            got[w] = load(p)
    pieces = {}
    if os.path.exists(os.path.join(d, "pieces.txt")):
        for l in open(os.path.join(d, "pieces.txt")):
            p = l.split()
            if len(p) == 2 and p[1].isdigit():
                pieces[p[0]] = int(p[1])
    s = {}
    bad_verify = []
    for w, ph in got.items():
        ck = [x for x in ph if x.get("phase") == "ckpt"]
        F("%s_checkpoints" % w, str(len(ck)))
        if not ck:
            continue
        lw = [x["longest_write_ms"] for x in ck]
        s[w] = {"longest": max(lw), "longest_min": min(lw), "op_max": max(x["op_s"] for x in ck),
                "op_min": min(x["op_s"] for x in ck), "pause": max(x["pause_ms"] for x in ck),
                "in_clone": max(x["in_clone_ms"] for x in ck), "n": len(ck)}
        M("%s_longest_write_ms" % w, max(lw), "ms", "longest single write overlapping any of %d checkpoints" % len(ck))
        M("%s_longest_write_min_ms" % w, min(lw), "ms", "the smallest of the per-checkpoint longest writes")
        M("%s_checkpoint_op_max_s" % w, s[w]["op_max"], "s", "the longest checkpoint, start to published")
        M("%s_checkpoint_op_min_s" % w, s[w]["op_min"], "s")
        M("%s_pause_max_ms" % w, s[w]["pause"], "ms", "the computer stopped (or the moment waited) at most")
        if w != "full":
            M("%s_in_clone_max_ms" % w, s[w]["in_clone"], "ms",
              "longest write while the last checkpoint (sharing blocks with the live image) was cloned" if w in ("cbw", "nbd")
              else "longest write while the empty delta file was made")
            M("%s_after_clone_max_ms" % w, max(x["after_clone_ms"] for x in ck), "ms", "longest write while the blocks were copied")
            M("%s_copied_mib_max" % w, max(x["copied_mib"] for x in ck), "MiB")
        for x in ph:
            p = x.get("phase")
            if p == "baseline":
                M("%s_baseline_p50_ms" % w, x["p50_ms"], "ms", "the stand-in computer's usual 4 KiB uncached write, no checkpoint")
                M("%s_baseline_max_ms" % w, x["max_ms"], "ms", "the disk's own noise: the longest write in 2 s with no checkpoint")
                s[w]["noise"] = x["max_ms"]
            elif p == "unrelated-clone":
                M("%s_unrelated_clone_longest_ms" % w, x["longest_write_ms"], "ms",
                  "longest live write while a checkpoint that shares blocks with it, and that nobody writes, was cloned")
                M("%s_unrelated_clone_s" % w, x["op_s"], "s")
                s[w]["unrelated"] = x["longest_write_ms"]
            elif p == "run":
                M("%s_run_p99_ms" % w, x["p99_ms"], "ms", "all writes of the run")
        drops = [x["longest_write_ms_3s"] for x in ph if x.get("phase") == "drop"]
        if drops:
            M("%s_drop_longest_ms" % w, max(drops), "ms", "longest write in the 3 s after a checkpoint was dropped")
            s[w]["drop"] = max(drops)
        ver = [x for x in ph if x.get("phase") == "verify"]
        ok = sum(1 for x in ver if x.get("ok"))
        F("%s_verify" % w, "%d of %d checkpoints held exactly the writes up to one moment" % (ok, len(ver)))
        if ok < len(ver):
            bad_verify.append("%s %d of %d" % (w, ok, len(ver)))
        if w in pieces:
            M("%s_live_pieces" % w, pieces[w], "pieces", "the live image's extents at the end of the run (the kit's extents probe)")

    crash = None
    cp = os.path.join(d, "crash.jsonl")
    if os.path.exists(cp):
        r = load(cp)
        crash = r[-1] if r else {"ok": False, "error": "no result line"}
        F("crash", json.dumps(crash, sort_keys=True))
    mutants = []
    mp = os.path.join(d, "mutants.txt")
    if os.path.exists(mp):
        mutants = [l.strip() for l in open(mp) if l.strip()]
        F("mutants", "; ".join(mutants))

    def g(w, k):
        return s.get(w, {}).get(k)

    shared = [(v, lab) for v, lab in ((g("cbw", "in_clone"), "cbw's clone of the last checkpoint"),
                                       (g("nbd", "in_clone"), "nbd's clone of the last checkpoint"),
                                       (g("cbw", "unrelated"), "cbw's clone of an older checkpoint"),
                                       (g("nbd", "unrelated"), "nbd's clone of an older checkpoint"),
                                       (g("full", "unrelated"), "full's clone of an older checkpoint")) if v is not None]
    noise = max([g(w, "noise") for w in s if g(w, "noise") is not None] or [0])
    parts = []
    for w in WAYS:
        if w in s:
            t = "%s: longest write %.0f to %.0f ms, checkpoint %.2f to %.2f s" % (
                w, s[w]["longest_min"], s[w]["longest"], s[w]["op_min"], s[w]["op_max"])
            if "drop" in s[w]:
                t += ", drop %.0f ms" % s[w]["drop"]
            parts.append(t)
    sentence = "; ".join(parts)
    if shared:
        sv, sl = max(shared)
        sentence += "; cloning a file that shares blocks with the live image made a write wait %.0f ms (%s); the disk's own noise %.0f ms" % (sv, sl, noise)
    if pieces:
        sentence += "; live image in %s pieces" % ", ".join("%d (%s)" % (v, k) for k, v in sorted(pieces.items()))
    xfs = " (XFS: full 9.2 to 11.8 s, cbw 0.16 to 0.31 s, up to 1.0 s for a shared clone)"
    if bad_verify:
        key = "not-a-moment"
        sentence = "checkpoints that did not verify: %s; %s" % (", ".join(bad_verify), sentence)
    elif (crash is not None and not crash.get("ok")) or any("NOT caught" in m for m in mutants):
        key = "catalog-broke"
        sentence = "catalog: %s, mutants: %s; %s" % (json.dumps(crash), "; ".join(mutants) or "not run", sentence)
    elif not shared or "full" not in s:
        key = "inconclusive"
        sentence = "not every way ran: " + (sentence or "nothing measured")
    elif s["full"]["longest"] <= 100:
        key = "full-free"
    else:
        sv = max(shared)[0]
        key = "shared-clone-free" if sv <= 100 else ("shared-clone-waits" if sv <= 1000 else "shared-clone-stalls")
    lines.append("O\t%s\t%s" % (key, sentence + xfs))
    print("\n".join(lines))


if __name__ == "__main__":
    main(sys.argv[1])

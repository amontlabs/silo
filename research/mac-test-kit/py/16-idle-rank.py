"""Test 16: summarise an idle night.

16-idle-rank.py SAMPLES.tsv CHANGES_DIR OUT_DIR

SAMPLES.tsv: one line per sample, tab separated, with a header:
  n t silovm_time vz_time vz_rss_kb vz_count disk_blocks df_free_kb guest_used_kb guest_mem_free_pct find_s changed
  (ps "time" fields are [[dd-]hh:]mm:ss.ss; a field is empty when it could not be read)
CHANGES_DIR: changes-NNN.txt, the paths the guest changed in the window that ended at sample NNN.

Writes OUT_DIR/summary.json (the numbers the test records) and OUT_DIR/ranked.txt (the folders
macOS writes by itself, most often first). Python 3.9, standard library only.
"""
import glob
import json
import os
import statistics
import sys

# Paths the kit itself causes: its marker, its SSH logins.
SELF = ("/private/var/tmp/silokit-16", "/var/tmp/silokit-16")


def cpu_seconds(s):
    s = (s or "").strip()
    if not s:
        return None
    days = 0
    if "-" in s:
        d, s = s.split("-", 1)
        days = int(d)
    parts = [float(p) for p in s.split(":")]
    secs = 0.0
    for p in parts:
        secs = secs * 60 + p
    return days * 86400 + secs


def num(s):
    try:
        return float(s)
    except (TypeError, ValueError):
        return None


def folder(path, depth):
    parts = [p for p in path.split("/") if p]
    if len(parts) <= 1:
        return "/" + "/".join(parts)
    return "/" + "/".join(parts[: min(depth, len(parts) - 1)])


def main():
    samples_path, changes_dir, out_dir = sys.argv[1:4]
    depth = int(os.environ.get("KIT_RANK_DEPTH", "4"))
    rows = []
    with open(samples_path) as f:
        header = f.readline().rstrip("\n").split("\t")
        for line in f:
            vals = line.rstrip("\n").split("\t")
            rows.append(dict(zip(header, vals + [""] * (len(header) - len(vals)))))

    intervals = []
    for a, b in zip(rows, rows[1:]):
        dt = num(b["t"]) - num(a["t"])
        if not dt or dt <= 0:
            continue
        iv = {"from": int(a["n"]), "to": int(b["n"]), "seconds": dt, "with_find": bool(b.get("find_s"))}
        for key in ("silovm_time", "vz_time"):
            ca, cb = cpu_seconds(a.get(key)), cpu_seconds(b.get(key))
            iv[key + "_pct"] = (cb - ca) / dt * 100 if ca is not None and cb is not None and cb >= ca else None
        intervals.append(iv)

    def median_of(key, quiet_only):
        vals = [iv[key] for iv in intervals if iv[key] is not None and (not quiet_only or not iv["with_find"])]
        return round(statistics.median(vals), 2) if vals else None

    def series(key):
        return [num(r.get(key)) for r in rows if num(r.get(key)) is not None]

    rss = series("vz_rss_kb")
    blocks = series("disk_blocks")
    free = series("df_free_kb")
    used = series("guest_used_kb")
    hours = (num(rows[-1]["t"]) - num(rows[0]["t"])) / 3600 if len(rows) > 1 else 0

    # Changed files: per window, then per folder over the night.
    windows = sorted(glob.glob(os.path.join(changes_dir, "changes-*.txt")))
    per_folder = {}
    per_path_windows = {}
    counts = []
    for w in windows:
        seen_here = set()
        n = 0
        for line in open(w, errors="replace"):
            p = line.rstrip("\n")
            if not p or p.startswith(SELF):
                continue
            n += 1
            per_path_windows[p] = per_path_windows.get(p, 0) + 1
            fo = folder(p, depth)
            e = per_folder.setdefault(fo, {"files": 0, "windows": 0, "paths": set()})
            e["files"] += 1
            e["paths"].add(p)
            if fo not in seen_here:
                e["windows"] += 1
                seen_here.add(fo)
        counts.append(n)

    ranked = sorted(per_folder.items(), key=lambda kv: (-kv[1]["windows"], -kv[1]["files"], kv[0]))
    nwin = len(windows)
    with open(os.path.join(out_dir, "ranked.txt"), "w") as out:
        out.write("# Folders the idle computer wrote by itself, most often first.\n")
        out.write("# windows = in how many of the %d windows the folder changed; files = changed files summed\n" % nwin)
        out.write("# over the windows; distinct = different files. Paths are as seen from / (Data volume).\n")
        out.write("%-8s %-8s %-9s %s\n" % ("windows", "files", "distinct", "folder"))
        for fo, e in ranked:
            out.write("%-8d %-8d %-9d %s\n" % (e["windows"], e["files"], len(e["paths"]), fo))
        out.write("\n# Files changed in every window\n")
        for p, c in sorted(per_path_windows.items()):
            if nwin and c == nwin:
                out.write(p + "\n")

    later = counts[1:] if len(counts) > 1 else counts
    summary = {
        "samples": len(rows),
        "hours": round(hours, 2),
        "vz_cpu_pct_median_quiet": median_of("vz_time_pct", True),
        "vz_cpu_pct_median_all": median_of("vz_time_pct", False),
        "silovm_cpu_pct_median_quiet": median_of("silovm_time_pct", True),
        "vz_rss_mb_first": round(rss[0] / 1024, 1) if rss else None,
        "vz_rss_mb_last": round(rss[-1] / 1024, 1) if rss else None,
        "vz_rss_mb_max": round(max(rss) / 1024, 1) if rss else None,
        "disk_allocated_mb_growth": round((blocks[-1] - blocks[0]) * 512 / 2**20, 1) if blocks else None,
        "mac_free_mb_change": round((free[-1] - free[0]) / 1024, 1) if free else None,
        "guest_used_mb_growth": round((used[-1] - used[0]) / 1024, 1) if used else None,
        "find_windows": nwin,
        "changed_files_first_window": counts[0] if counts else None,
        "changed_files_per_window_median_after_first": statistics.median(later) if later else None,
        "folders": len(per_folder),
        "top_folders": [fo for fo, _ in ranked[:10]],
        "vz_count_max": max(series("vz_count")) if series("vz_count") else None,
    }
    json.dump(summary, open(os.path.join(out_dir, "summary.json"), "w"), indent=1)
    print(json.dumps(summary))


if __name__ == "__main__":
    main()

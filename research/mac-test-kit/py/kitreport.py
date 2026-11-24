"""Reads the tests' headers and a run's results.jsonl. Used by ./kit report and ./kit doc.

kitreport.py report KIT_ROOT RESULTS_DIR   writes RESULTS_DIR/summary.md and prints it
kitreport.py doc KIT_ROOT                   rewrites KIT_ROOT/RESULTS.md
kitreport.py check KIT_ROOT                 checks every test's header; exit 1 on a problem
"""
import glob
import json
import os
import re
import sys

FIELDS = ["id", "title", "tier", "time", "needs", "decides", "question"]
TIERS = {"setup", "quick", "vm", "guided", "long"}


def headers(root):
    tests = []
    for path in sorted(glob.glob(os.path.join(root, "tests", "[0-9][0-9]-*.sh"))):
        h = {"path": os.path.relpath(path, root), "outcomes": []}
        for line in open(path):
            m = re.match(r"^# (\w+): (.*)$", line.rstrip("\n"))
            if m and m.group(1) in FIELDS and m.group(1) not in h:
                h[m.group(1)] = m.group(2).strip()
            m = re.match(r"^# outcome ([a-z0-9-]+): (.*)$", line.rstrip("\n"))
            if m:
                h["outcomes"].append((m.group(1), m.group(2).strip()))
            m = re.match(r"^#   (.*)$", line.rstrip("\n"))
            if m and h["outcomes"] and not line.startswith("# outcome"):
                k, v = h["outcomes"][-1]
                h["outcomes"][-1] = (k, v + " " + m.group(1).strip())
        tests.append(h)
    return tests


def check(root):
    problems = []
    seen = set()
    for h in headers(root):
        for f in FIELDS:
            if not h.get(f):
                problems.append("%s: no '# %s:' line" % (h["path"], f))
        if h.get("tier") not in TIERS:
            problems.append("%s: tier %r is not one of %s" % (h["path"], h.get("tier"), sorted(TIERS)))
        if h.get("id") in seen:
            problems.append("%s: id %s is used twice" % (h["path"], h.get("id")))
        seen.add(h.get("id"))
        if not os.path.basename(h["path"]).startswith(h.get("id", "?") + "-"):
            problems.append("%s: file name does not start with its id" % h["path"])
        if not h["outcomes"]:
            problems.append("%s: no '# outcome key: meaning' line" % h["path"])
        src = open(os.path.join(root, h["path"])).read()
        keys = {k for k, _ in h["outcomes"]}
        for used in re.findall(r"kit_outcome\s+\"?([a-z0-9-]+)", src):
            if used not in keys:
                problems.append("%s: kit_outcome %s has no '# outcome %s:' line" % (h["path"], used, used))
    for p in problems:
        print(p)
    return 1 if problems else 0


def doc(root):
    out = [
        "# What each result means",
        "",
        "Written by `./kit doc` from the `# outcome` lines at the top of each test, which are the",
        "only place these meanings are kept. Each test ends on one outcome; `./kit report` prints it",
        "next to the line below, so a run's summary says which ruling it moves.",
        "",
        "The entries named are entries of the plan (`plan show <id>`) or the research of",
        "24 November 2026 (`research/2026-11-24-first-build-decisions/`). A result moves nothing by",
        "itself: it is evidence for the owner, or for agents on an entry agents may take.",
        "",
    ]
    for h in headers(root):
        out += [
            "## %s. %s" % (h.get("id"), h.get("title")),
            "",
            h.get("question", ""),
            "",
            "- Decides: %s" % h.get("decides", ""),
            "- Tier: `%s`. Time: %s. Needs: %s." % (h.get("tier"), h.get("time"), h.get("needs")),
            "- Script: [`%s`](%s)" % (h["path"], h["path"]),
            "",
            "| Outcome | What it means |",
            "|---|---|",
        ]
        for k, v in h["outcomes"]:
            out.append("| `%s` | %s |" % (k, v.replace("|", "\\|")))
        out.append("")
    text = "\n".join(out)
    open(os.path.join(root, "RESULTS.md"), "w").write(text)
    print("wrote RESULTS.md (%d tests)" % len(headers(root)))


def report(root, rdir):
    meta = {h["id"]: h for h in headers(root)}
    events = []
    path = os.path.join(rdir, "results.jsonl")
    for line in open(path):
        try:
            events.append(json.loads(line))
        except ValueError:
            pass
    host = {}
    try:
        host = json.load(open(os.path.join(rdir, "host.json")))
    except (OSError, ValueError):
        pass
    order = []
    by = {}
    for e in events:
        t = e.get("test")
        if t not in by:
            by[t] = []
            order.append(t)
        by[t].append(e)
    out = ["# Silo Mac kit run %s" % os.path.basename(os.path.realpath(rdir)), ""]
    out.append("Host: macOS %s (%s), %s, %s GB, FileVault: %s" % (
        host.get("macos", host.get("system")), host.get("build", host.get("kernel")),
        host.get("chip", host.get("machine")), host.get("memory_gb", "?"), host.get("filevault", "?")))
    out.append("")
    out.append("| Test | Result | Means |")
    out.append("|---|---|---|")
    for t in order:
        ev = by[t]
        m = meta.get(t, {})
        means = dict(m.get("outcomes", []))
        oc = [e for e in ev if e["kind"] == "outcome"]
        sk = [e for e in ev if e["kind"] == "skip"]
        er = [e for e in ev if e["kind"] == "error"]
        if oc:
            res = "**%s**: %s" % (oc[-1]["key"], oc[-1]["text"])
            mean = means.get(oc[-1]["key"], "(no meaning written for this outcome)")
        elif er:
            res, mean = "error: %s" % er[-1]["reason"], "The test broke; see logs/%s.log" % t
        elif sk:
            res, mean = "skipped: %s" % sk[-1]["reason"], ""
        else:
            res, mean = "no outcome", "See logs/%s.log" % t
        out.append("| %s %s | %s | %s |" % (t, m.get("title", ""), res.replace("|", "\\|"), mean.replace("|", "\\|")))
    out.append("")
    for t in order:
        ev = by[t]
        ms = [e for e in ev if e["kind"] in ("measure", "fact")]
        if not ms:
            continue
        out.append("## %s %s" % (t, meta.get(t, {}).get("title", "")))
        out.append("")
        for e in ms:
            if e["kind"] == "measure":
                out.append("- %s = %s %s%s" % (e["name"], e["value"], e["unit"], (" (%s)" % e["context"]) if e.get("context") else ""))
            else:
                out.append("- %s: %s" % (e["name"], e["value"]))
        out.append("")
    text = "\n".join(out)
    open(os.path.join(rdir, "summary.md"), "w").write(text)
    print(text)


if __name__ == "__main__":
    if sys.argv[1] == "report":
        report(sys.argv[2], sys.argv[3])
    elif sys.argv[1] == "doc":
        doc(sys.argv[2])
    elif sys.argv[1] == "check":
        sys.exit(check(sys.argv[2]))

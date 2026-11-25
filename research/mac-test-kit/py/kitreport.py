"""Reads the tests' headers and a run's results.jsonl. Used by ./kit report and ./kit doc.

kitreport.py report KIT_ROOT RESULTS_DIR   writes RESULTS_DIR/summary.md and prints it
kitreport.py doc KIT_ROOT                   rewrites KIT_ROOT/RESULTS.md
kitreport.py check KIT_ROOT                 checks every test's header and decisions.txt; exit 1 on a problem
kitreport.py order KIT_ROOT [WHAT...]       the tests to run, in the kit's order (paths)
kitreport.py verdicts KIT_ROOT DIR...       one verdict per decision, from the newest outcome of each
                                            test across the result folders given
"""
import glob
import json
import os
import re
import sys

FIELDS = ["id", "title", "tier", "time", "needs", "step", "decides", "question"]
TIERS = {"setup", "quick", "vm", "guided", "long"}
TIER_ORDER = {"setup": 0, "quick": 1, "vm": 2, "guided": 3, "long": 4}
# A test's step is the step of the first build (#348, specified in #370) it must run before, or
# that it finishes; "later" for the second build and after.
STEPS = {"0", "1", "2", "3", "4", "5", "6", "later"}


def headers(root):
    tests = []
    for path in sorted(glob.glob(os.path.join(root, "tests", "[0-9][0-9]-*.sh"))):
        h = {"path": os.path.relpath(path, root), "outcomes": []}
        for line in open(path):
            m = re.match(r"^# (\w+): (.*)$", line.rstrip("\n"))
            if m and m.group(1) in FIELDS + ["blocks"] and m.group(1) not in h:
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
        if h.get("step") and h["step"] not in STEPS:
            problems.append("%s: step %r is not one of %s" % (h["path"], h["step"], sorted(STEPS)))
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
    problems += check_decisions(root)
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
        ] + (["- **Blocks: %s.**" % h["blocks"]] if h.get("blocks") else []) + [
            "- Tier: `%s`. Step: %s. Time: %s. Needs: %s." % (h.get("tier"), step_text(h.get("step")), h.get("time"), h.get("needs")),
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
    open(os.path.join(root, "DECISIONS.md"), "w").write(doc_decisions(root))
    print("wrote RESULTS.md (%d tests) and DECISIONS.md (%d decisions)" % (len(headers(root)), len(decisions(root))))


def step_text(step):
    if step == "later":
        return "after the first build"
    if step == "0":
        return "0, the kit itself"
    return "needed for step %s of the first build" % step


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
        title = m.get("title", "") + (" (blocks %s)" % m["blocks"] if m.get("blocks") else "")
        out.append("| %s %s | %s | %s |" % (t, title, res.replace("|", "\\|"), mean.replace("|", "\\|")))
    out.append("")
    table_end = len(out)
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
    try:
        vtext, _ = verdicts(root, [rdir], out=False)
        out[table_end:table_end] = [
            vtext.replace("## One verdict per decision", "## One verdict per decision, from this run alone"),
            "Across every run kept: `./kit verdicts`.", ""]
    except (OSError, ValueError) as e:
        out[table_end:table_end] = ["(no verdicts: %s)" % e, ""]
    text = "\n".join(out)
    open(os.path.join(rdir, "summary.md"), "w").write(text)
    print(text)


# --- The order tests run in -------------------------------------------------------------------

def order_key(h):
    """setup first; then what blocks a step, unless it takes a night; then by the step of the first
    build it is needed for, cheapest tier first; then by id."""
    step = h.get("step", "later")
    step_n = 9 if step == "later" else int(step)
    blocking = 0 if h.get("blocks") and h.get("tier") != "long" else 1
    setup = 0 if h.get("tier") == "setup" else 1
    return (setup, blocking, step_n, TIER_ORDER.get(h.get("tier"), 9), h.get("id"))


def ordered(root, what=None):
    """The tests matching WHAT (ids, tiers, "blocking", "all"; all by default) in the kit's order."""
    hs = headers(root)
    if what:
        keep = []
        for h in hs:
            for w in what:
                if w == "all" or w == h.get("id") or w == h.get("tier") or (w == "blocking" and h.get("blocks")) \
                        or (w.startswith("step") and w[4:] == h.get("step")):
                    keep.append(h)
                    break
        hs = keep
    return sorted(hs, key=order_key)


# --- Decisions: which open entry each outcome answers -------------------------------------------
#
# decisions.txt holds, for each open entry of the plan (or of a draft) that a Mac test answers, its
# options in the order recommended, and which options each outcome leaves possible. The verdict on
# an entry is the first option every answered test still leaves possible.

def decisions(root):
    out, cur = [], None
    for n, line in enumerate(open(os.path.join(root, "decisions.txt")), 1):
        raw = line.rstrip("\n")
        if not raw.strip() or raw.lstrip().startswith("#"):
            continue
        m = re.match(r"^decision ([a-z0-9-]+)$", raw)
        if m:
            cur = {"id": m.group(1), "line": n, "options": [], "rules": [], "tests": [], "source": "", "asks": ""}
            out.append(cur)
            continue
        if cur is None:
            raise ValueError("decisions.txt:%d: a line before the first 'decision'" % n)
        m = re.match(r"^  (source|asks): (.*)$", raw)
        if m:
            cur[m.group(1)] = m.group(2).strip()
            continue
        m = re.match(r"^  tests: ([0-9 ]+)$", raw)
        if m:
            cur["tests"] = m.group(1).split()
            continue
        m = re.match(r"^  option ([a-z0-9-]+): (.*)$", raw)
        if m:
            cur["options"].append((m.group(1), m.group(2).strip()))
            continue
        m = re.match(r"^  ([0-9]{2}) (fact [A-Za-z0-9_.]+=\S+|[a-z0-9-]+): ?(.*)$", raw)
        if m:
            cur["rules"].append({"test": m.group(1), "when": m.group(2), "leaves": m.group(3).split(), "line": n})
            continue
        raise ValueError("decisions.txt:%d: not understood: %s" % (n, raw))
    return out


def plan_ids(root):
    """Every entry id of the plan this kit sits in (../../plan), or None when there is none."""
    plan = os.path.normpath(os.path.join(root, "..", "..", "plan"))
    if not os.path.isdir(plan):
        return None
    ids = set()
    for p in glob.glob(os.path.join(plan, "**", "*.json"), recursive=True):
        ids.add(os.path.basename(p)[:-5])
    return ids


def check_decisions(root):
    problems = []
    try:
        ds = decisions(root)
    except (OSError, ValueError) as e:
        return [str(e)]
    hs = {h["id"]: h for h in headers(root)}
    ids = plan_ids(root)
    covered = set()
    seen = set()
    for d in ds:
        where = "decisions.txt:%d %s" % (d["line"], d["id"])
        if d["id"] in seen:
            problems.append("%s: listed twice" % where)
        seen.add(d["id"])
        if not d["asks"] or not d["source"]:
            problems.append("%s: needs 'asks:' and 'source:'" % where)
        if ids is not None and d["id"] not in ids and not re.search(r"#\d+", d["source"]):
            problems.append("%s: not an entry of the plan, and its source names no draft (#NNN)" % where)
        if not d["tests"]:
            problems.append("%s: no 'tests:' line" % where)
        opts = [o for o, _ in d["options"]]
        for t in d["tests"]:
            if t not in hs:
                problems.append("%s: test %s does not exist" % (where, t))
            covered.add(t)
        for r in d["rules"]:
            w = "decisions.txt:%d" % r["line"]
            if r["test"] not in d["tests"]:
                problems.append("%s: test %s is not in the decision's 'tests:' line" % (w, r["test"]))
            h = hs.get(r["test"])
            if h and not r["when"].startswith("fact "):
                if r["when"] not in dict(h["outcomes"]):
                    problems.append("%s: test %s has no outcome %s" % (w, r["test"], r["when"]))
                if r["when"] == "inconclusive":
                    problems.append("%s: an inconclusive outcome answers nothing; leave it out" % w)
            if not opts:
                problems.append("%s: a rule on a decision with no options" % w)
            if not r["leaves"]:
                problems.append("%s: leaves no option (write every option it leaves possible)" % w)
            for o in r["leaves"]:
                if o not in opts:
                    problems.append("%s: option %s is not an option of %s" % (w, o, d["id"]))
    for t in hs:
        if t not in covered and hs[t].get("tier") != "setup":
            problems.append("tests/%s: in no decision of decisions.txt" % t)
    return problems


def newest_results(dirs):
    """{test id: {"outcome": (key, text, t, run) or None, "facts": {name: value}, "skip": reason}},
    from the newest outcome of each test across the runs."""
    by = {}
    for d in dirs:
        path = os.path.join(d, "results.jsonl")
        if not os.path.isfile(path):
            continue
        run = os.path.basename(os.path.realpath(d))
        for line in open(path):
            try:
                e = json.loads(line)
            except ValueError:
                continue
            t = e.get("test")
            r = by.setdefault(t, {"outcome": None, "facts": {}, "fact_t": {}, "skip": None})
            if e.get("kind") == "outcome" and (r["outcome"] is None or e["t"] >= r["outcome"][2]):
                r["outcome"] = (e["key"], e.get("text", ""), e["t"], run)
            elif e.get("kind") == "fact" and e["t"] >= r["fact_t"].get(e["name"], 0):
                r["facts"][e["name"]] = str(e["value"])
                r["fact_t"][e["name"]] = e["t"]
            elif e.get("kind") == "skip":
                r["skip"] = e.get("reason", "")
    return by


def verdict(d, res):
    """One decision against the results: a dict with status, option, text, answered, waiting."""
    opts = [o for o, _ in d["options"]]
    possible = list(opts)
    answered, waiting, said = [], [], []
    for t in d["tests"]:
        r = res.get(t)
        oc = r and r["outcome"]
        if not oc or oc[0] == "inconclusive":
            waiting.append(t + (" (inconclusive)" if oc else ""))
            continue
        answered.append("%s %s" % (t, oc[0]))
        for rule in d["rules"]:
            if rule["test"] != t:
                continue
            w = rule["when"]
            if w.startswith("fact "):
                name, _, value = w[5:].partition("=")
                if r["facts"].get(name) != value:
                    continue
            elif w != oc[0]:
                continue
            said.append(("%s %s" % (t, w), rule["leaves"]))
            possible = [o for o in possible if o in rule["leaves"]]
    v = {"id": d["id"], "answered": answered, "waiting": waiting}
    if not opts:
        v["status"] = "measured" if not waiting else ("waiting" if not answered else "partly measured")
        v["option"] = None
        v["text"] = "measured by %s" % ", ".join(answered) if answered else "no test answered yet"
        return v
    if not possible:
        v["status"], v["option"] = "conflict", None
        v["text"] = "the tests leave no option: " + "; ".join("%s leaves %s" % (k, " ".join(o)) for k, o in said)
        return v
    v["option"] = possible[0]
    desc = dict(d["options"])[possible[0]]
    others = possible[1:]
    if not answered:
        v["status"] = "waiting"
        v["text"] = "%s (recommended; no test answered yet)" % possible[0]
    elif waiting:
        v["status"] = "leaning"
        v["text"] = "%s: %s" % (possible[0], desc)
    else:
        v["status"] = "settled" if not others else "answered"
        v["text"] = "%s: %s" % (possible[0], desc)
    if others:
        v["text"] += " (still possible: %s)" % ", ".join(others)
    return v


def verdicts(root, dirs, out=None):
    res = newest_results(dirs)
    rows = [verdict(d, res) for d in decisions(root)]
    lines = [
        "## One verdict per decision",
        "",
        "From the newest outcome of each test in %s. `settled`: every test that bears on it answered and "
        "one option is left. `answered`: every test answered, and the first option left is the "
        "recommended one. `leaning`: some tests have not answered yet. `conflict`: two tests leave no "
        "option in common. Mappings: [decisions.txt](decisions.txt) of the kit." % (
            "this run" if len(dirs) == 1 else "%d runs" % len(dirs)),
        "",
        "| Decision | Status | Verdict | Answered by | Not yet |",
        "|---|---|---|---|---|",
    ]
    for v in rows:
        lines.append("| `%s` | %s | %s | %s | %s |" % (
            v["id"], v["status"], v["text"].replace("|", "\\|"), ", ".join(v["answered"]) or "",
            ", ".join(v["waiting"]) or ""))
    lines.append("")
    text = "\n".join(lines)
    if out is None:
        print(text)
    return text, rows


def doc_decisions(root):
    out = [
        "# By decision",
        "",
        "Every open entry a test of the kit answers, with the options in the order recommended and",
        "what each outcome leaves possible. The verdict of a run is the first option every answered",
        "test leaves possible (`./kit verdicts`). Written by `./kit doc` from",
        "[decisions.txt](decisions.txt), the only place these mappings are kept.",
        "",
    ]
    hs = {h["id"]: h for h in headers(root)}
    for d in decisions(root):
        out += ["## `%s`" % d["id"], "", d["asks"], "", "- Source: %s" % d["source"],
                "- Tests: %s" % ", ".join("%s (%s)" % (t, hs[t]["title"]) if t in hs else t for t in d["tests"]), ""]
        if d["options"]:
            out += ["| Option | Means |", "|---|---|"]
            for o, desc in d["options"]:
                out.append("| `%s` | %s |" % (o, desc.replace("|", "\\|")))
            out.append("")
            if d["rules"]:
                out += ["| Test and outcome | Leaves possible |", "|---|---|"]
                for r in d["rules"]:
                    out.append("| %s `%s` | %s |" % (r["test"], r["when"], ", ".join("`%s`" % o for o in r["leaves"])))
                out.append("")
            other = [t for t in d["tests"] if t not in {r["test"] for r in d["rules"]}]
            if other:
                out += ["Tests %s give evidence (their numbers and facts) and rule out no option." % ", ".join(other), ""]
        else:
            out += ["Measured, not chosen: the verdict is the measures of the tests above.", ""]
    return "\n".join(out)


if __name__ == "__main__":
    if sys.argv[1] == "report":
        report(sys.argv[2], sys.argv[3])
    elif sys.argv[1] == "doc":
        doc(sys.argv[2])
    elif sys.argv[1] == "check":
        sys.exit(check(sys.argv[2]))
    elif sys.argv[1] == "order":
        for h in ordered(sys.argv[2], sys.argv[3:]):
            print(os.path.join(sys.argv[2], h["path"]))
    elif sys.argv[1] == "verdicts":
        verdicts(sys.argv[2], sys.argv[3:])

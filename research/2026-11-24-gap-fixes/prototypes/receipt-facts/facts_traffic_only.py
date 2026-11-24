"""The same receipt facts with no probe: only the model traffic and the disk checkpoints, which
are both seen from outside. This is what a macOS computer gets until reports from inside exist
there (`macos-ships-facts-first`). What is lost is said, never guessed.
Usage: python3 facts_traffic_only.py <session dir> > facts_traffic_only.json
"""
import json, os, re, shlex, sys
import facts as F

TEST_WORDS = re.compile(r"^(pytest|py\.test|jest|vitest|mocha|tox|nox)$")


def segments(command):
    """Split a shell command into pipelines and their members, at the top level only. A command
    hidden in a script or a variable is not seen: that is the probe's job."""
    command = re.sub(r"\d*>&\d+|\d*>>?\s*[^\s|;&]+|<<-?\s*'?\w+'?|<\s*[^\s|;&]+", " ", command)
    try:
        lex = shlex.shlex(command, posix=True, punctuation_chars=";&|")
        lex.whitespace_split = True
        toks = list(lex)
    except ValueError:
        return []
    pls, cur, member = [], [], []
    for t in toks:
        if t == "|":
            cur.append(member); member = []
        elif t in (";", "&&", "||", "&", "\n"):
            cur.append(member); pls.append(cur); cur, member = [], []
        else:
            member.append(t)
    cur.append(member); pls.append(cur)
    return [[m for m in pl if m] for pl in pls if any(pl)]


def test_command(argv):
    r, rest = F.runner_of(argv)
    if r:
        return r, F.selectors(r, rest)
    a = [os.path.basename(x) for x in argv]
    if a and a[0] in ("npm", "pnpm", "yarn", "bun") and len(a) > 1 and (a[1] == "test" or (a[1] == "run" and len(a) > 2 and a[2].startswith("test"))):
        return f"{a[0]} script (runner not seen)", [{"selector": "script", "value": " ".join(argv[1:]), "means": "what the script runs is not seen"}] if a[1] == "run" and a[2] != "test" else []
    if a and a[0] == "make" and any("test" in x for x in a[1:]):
        return "make target (runner not seen)", []
    if a and TEST_WORDS.match(a[0]):
        return a[0], []
    return None, []


def main(session):
    project = os.path.join(session, "project")
    reqs, calls, results = F.load_traffic(os.path.join(session, "requests.jsonl"))
    cps = F.load_checkpoints(os.path.join(session, "checkpoints.jsonl"))
    tests = []
    for cid, c in calls.items():
        res = results.get(cid, {})
        for pl in segments(c["command"] or ""):
            for i, member in enumerate(pl):
                r, sel = test_command(member)
                if not r:
                    continue
                counts = F.shown_counts(res.get("text") or "")
                failed = (counts or {}).get("failed", 0) + (counts or {}).get("error", 0)
                if counts is None:
                    out = "unknown: no summary in what the agent was shown"
                elif failed:
                    out = "failed, as the output shown says"
                else:
                    out = "passed, as the output shown says"
                t = {"tool_call": cid, "command": c["command"], "runner": r,
                     "piped_into": [" ".join(m) for m in pl[i + 1:]],
                     "is_error_shown": res.get("is_error"),
                     "outcome": {"kind": "fact", "source": "traffic", "value": out,
                                 "note": "the output was shown to the agent; what it says is the runner's word"},
                     "masked": {"kind": "fact", "source": "traffic",
                                "means": "the output shown says tests failed, and the agent program marked the command as not failed",
                                "value": bool(failed) and res.get("is_error") is False},
                     "narrowed": {"kind": "opinion", "source": "built-in classifier over the command text",
                                  "value": bool(sel), "selectors": sel},
                     "counts_shown": {"kind": "fact", "source": "traffic", "value": counts},
                     "read": {"kind": "report", "source": "probe", "value": [], "missing": "no probe"}}
                after = cps[res["request"]] if res.get("request") is not None else None
                ch = F.changed(after, cps[-1]) if after else []
                ign = F.ignored(project, [x["path"] for x in ch])
                ch = [x for x in ch if x["path"] not in ign and not x["path"].startswith(".git/")]
                t["changed_after"] = {"kind": "fact", "source": "checkpoints",
                                      "between": [after["label"], cps[-1]["label"]] if after else None, "value": ch}
                t["passed"] = out.startswith("passed")
                t["stale"] = t["passed"] and bool(ch)
                t["pid"] = 0
                t["runner_exit"] = {"kind": "report", "source": "probe", "value": None, "missing": "no probe"}
                tests.append(t)
    weak = F.weakening(cps[0], cps[-1])
    for cid, c in calls.items():
        for pl in segments(c["command"] or ""):
            for m in pl:
                if m and os.path.basename(m[0]) == "git" and ("--no-verify" in m or ("commit" in m and "-n" in m)):
                    weak.append({"what": "hook bypass flag in a command the model asked for", "command": " ".join(m)})
    claim = F.final_text(session)
    return {"probe": "none",
            "tests": tests,
            "weakening": {"kind": "opinion", "source": "built-in classifier over the change between the first and last checkpoint, and the command text", "value": weak},
            "claim": {"kind": "fact", "source": "traffic", "value": claim},
            "claim_against_record": {"kind": "opinion", "source": "built-in classifier", **match(claim, tests)}}


def match(claim, tests):
    # same labels as with the probe; an unknown outcome is not a pass
    for t in tests:
        t["outcome"]["value"] = t["outcome"]["value"]
    adapted = []
    for t in tests:
        o = t["outcome"]["value"]
        v = "passed" if o.startswith("passed") else ("failed" if o.startswith("failed") else "output cut, outcome unknown")
        adapted.append(dict(t, outcome={"value": v}))
    return F.match_claim(claim, adapted)


if __name__ == "__main__":
    json.dump(main(sys.argv[1]), sys.stdout, indent=1, default=str)

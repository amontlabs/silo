"""The classifiers that ship with the prototype. Each gives opinions computed from facts and reports,
never written over them, each under its name and version:

  which-run-did-it@0.1   process -> run, conversation -> run, tool call -> process, file -> run
  how-each-run-is-going@0.1   working, waiting on the model, waiting on a command, idle, stuck
  claims@0.1   the agent's last words checked against the record: matched, mismatch, not checked

A process belongs to a run when a walk up its parents reaches the launch of an agent program
(`matching-reports-to-facts`). A file change is a run's when the probe's file reports between the
two checkpoints of the fact come from that run alone; from several runs, or none, it is
unattributed, and the runs that reported it are named.
"""
import os, re, json, time, collections
import record as R

WHICH = "which-run-did-it@0.1"
HOW_GOING = "how-each-run-is-going@0.1"
CLAIMS = "claims@0.1"

TEST_CMD = re.compile(r"(^|[\s;&|(])(node\s+--test|npm\s+(run\s+)?test|pytest|bun\s+test|cargo\s+test|go\s+test|jest|vitest|/bun\s+test)(\s|$)")
FILTERED = re.compile(r"(--test|test)\s+(?!--)\S*[/.*]")  # a test command given files or a folder
AGENTS = {"claude": "claude", "codex": "codex"}


def prog_name(program, command):
    base = os.path.basename(program or "")
    for k in AGENTS:
        if k in base or (command or "").split(" ")[0].endswith(k):
            return k
    return base


def own_file(p):
    """A file the agent program keeps for itself: its sessions, caches, logs. Its settings that
    every later run reads (CLAUDE.md, AGENTS.md) are not."""
    return bool(re.search(r"/\.(claude|codex)/|/\.claude\.json", p)) and not p.endswith(("CLAUDE.md", "AGENTS.md"))


class View:
    def __init__(self, root, live=False, now=None):
        self.root = root
        self.lines = R.read(root)
        self.by_id = {l["id"]: l for l in self.lines}
        self.now = now or time.time()
        self.live = live
        self.computers = sorted({l["computer"] for l in self.lines})
        cfg = json.load(open(os.path.join(root, "..", "night.json")))
        self.homes = {c: x["home"] for c, x in cfg["computers"].items()}
        self._procs()
        self._runs()
        self._convs()
        self._files()

    # ---------------------------------------------------------- facts and reports by kind
    def facts(self, line=None, computer=None):
        return [l for l in self.lines if l["kind"] == "fact" and (line is None or l["line"] == line) and (computer is None or l["computer"] == computer)]

    def reports(self, line=None, computer=None):
        return [l for l in self.lines if l["kind"] == "report" and (line is None or l["line"] == line) and (computer is None or l["computer"] == computer)]

    def control(self, what=None, computer=None):
        return [l for l in self.facts("control", computer) if what is None or l["what"] == what]

    def text(self, ref):
        return R.text(self.root, ref)

    def home(self, computer):
        cfg = json.load(open(os.path.join(self.root, "..", "night.json")))
        return cfg["computers"][computer]["home"]

    def tilde_text(self, computer, s):
        return s.replace(self.homes[computer], "~") if s else s

    def tilde(self, computer, p):
        h = self.home(computer)
        return "~" + p[len(h):] if p and p.startswith(h) else p

    # ---------------------------------------------------------- processes (reports)
    def _procs(self):
        self.procs = {}
        for l in self.reports("process"):
            key = (l["computer"], l["pid"])
            p = self.procs.setdefault(key, {"computer": l["computer"], "pid": l["pid"], "parent": None, "commands": [], "start": l["at"], "end": None, "exit": None, "launch": None, "ids": []})
            p["ids"].append(l["id"])
            if l["event"] == "forked":
                p["parent"] = l["parent"]; p["start"] = l["at"]; p["thread"] = bool(l.get("thread"))
            elif l["event"] == "started":
                p["commands"].append(l["command"]); p["program"] = l.get("program"); p["cwd"] = l.get("cwd")
                if l.get("launch"):
                    p["launch"] = l["launch"]
            elif l["event"] == "exited":
                p["end"] = l["at"]; p["exit"] = l.get("exit"); p["signal"] = l.get("signal")
        for p in self.procs.values():
            p["commands"] = [self.tilde_text(p["computer"], c) for c in p["commands"]]
            p["command"] = p["commands"][-1] if p["commands"] else None

    def run_of_pid(self, computer, pid, seen=None):
        """A walk up the parents to a launch. A broken chain belongs to no run."""
        seen = seen or set()
        p = self.procs.get((computer, pid))
        while p and p["pid"] not in seen:
            seen.add(p["pid"])
            if p["launch"]:
                return self.runs_by_launch.get(p["launch"])
            if p["parent"] is None:
                return None
            p = self.procs.get((computer, p["parent"]))
        return None

    def _runs(self):
        self.runs, self.runs_by_launch = [], {}
        counts = collections.Counter()
        for p in sorted(self.procs.values(), key=lambda p: p["start"]):
            if not p["launch"]:
                continue
            prog = prog_name(p.get("program"), p["commands"][0] if p["commands"] else "")
            counts[prog] += 1
            r = {"id": p["launch"], "computer": p["computer"], "program": prog, "name": f"{prog} #{counts[prog]}" if prog == "claude" else prog,
                 "pid": p["pid"], "start": p["start"], "end": p["end"], "exit": p["exit"], "signal": p.get("signal"), "cwd": p.get("cwd")}
            self.runs.append(r); self.runs_by_launch[r["id"]] = r
        if counts["codex"] > 1:
            n = 0
            for r in self.runs:
                if r["program"] == "codex":
                    n += 1; r["name"] = f"codex #{n}"
        for p in self.procs.values():
            p["run"] = self.run_of_pid(p["computer"], p["pid"])

    def runs_of(self, computer):
        return [r for r in self.runs if r["computer"] == computer]

    def run(self, name_or_id):
        for r in self.runs:
            if name_or_id in (r["id"], r["name"], r["name"].replace(" ", ""), r["name"].replace(" #", "#")):
                return r
        return None

    # ---------------------------------------------------------- conversations (facts), tied to runs (opinions)
    def _convs(self):
        owner = {}
        for l in self.reports("owner"):
            owner[l["about"]] = l["pid"]
        self.owner = owner
        self.calls = self.facts("model call")
        self.convs = collections.OrderedDict()
        for m in self.calls:
            cv = self.convs.setdefault(m["conversation"], {"id": m["conversation"], "computer": m["computer"], "calls": [], "runs": collections.Counter()})
            cv["calls"].append(m)
            pid = owner.get(m["id"])
            run = self.run_of_pid(m["computer"], pid) if pid else None
            m["_run"] = run
            cv["runs"][run["id"] if run else None] += 1
        for cv in self.convs.values():
            known = [k for k in cv["runs"] if k]
            cv["run"] = self.runs_by_launch[known[0]] if len(known) == 1 else None
            cv["run_rests_on"] = (f"the connections of its {sum(cv['runs'].values())} calls were held by processes of {cv['run']['name']}" if cv["run"] else
                                  "its calls' connections were held by processes of several runs" if len(known) > 1 else "no process report for its connections")
            cv["side"] = all(m.get("side") for m in cv["calls"])
            first = cv["calls"][0]
            cv["asked"] = self.text(first.get("you_asked_ref"))
        # tool calls with their window: from the reply that asked to the request that brought the result
        self.tool_calls = []
        for cv in self.convs.values():
            for i, m in enumerate(cv["calls"]):
                for tc in m.get("tool_calls") or []:
                    end, result = None, None
                    for n in cv["calls"][i + 1:]:
                        for rs in n.get("results_sent") or []:
                            if rs["tool_call"] == tc["id"]:
                                end, result = n["started"], rs
                        if end:
                            break
                    t = {**tc, "summary": self.tilde_text(m["computer"], tc["summary"]), "computer": m["computer"], "conversation": cv["id"], "turn": m["turn"], "asked_at": m["at"], "result_at": end,
                         "result": result, "call": m, "run": cv["run"]}
                    self.tool_calls.append(t)
        # tool call -> process. Time alone is not enough: the agent programs start processes of
        # their own (Claude Code runs git while a tool call is open), so the process that a tool
        # call started is the one whose command line holds the tool call's text, started while
        # the tool call was open; its descendants follow it.
        for p in self.procs.values():
            p["tool_call"] = None
        kids = collections.defaultdict(list)
        for p in self.procs.values():
            if p["parent"] is not None:
                kids[(p["computer"], p["parent"])].append(p)
        for t in self.tool_calls:
            if not t["run"] or t["tool"] not in ("Bash", "exec_command", "shell"):
                continue
            segs = [x.strip() for x in re.split(r"[\"'\\$`;&|]", " ".join((t["summary"] or "").split())) if x.strip() and not x.strip().startswith("cd ")]
            if not segs:
                continue
            key = max(segs, key=len)
            roots = [p for p in self.procs.values() if p["run"] is t["run"] and p["tool_call"] is None and t["asked_at"] <= p["start"]
                     and (t["result_at"] is None or p["start"] <= t["result_at"]) and any(key in " ".join(c.split()) for c in p["commands"])]
            roots.sort(key=lambda p: p["start"])
            if not roots:
                continue
            stack = [roots[0]]
            while stack:
                q = stack.pop()
                if q["tool_call"] is None:
                    q["tool_call"] = t
                    stack += kids[(q["computer"], q["pid"])]

    def run_calls(self, run, side=None):
        return [m for cv in self.convs.values() if cv["run"] is run and (side is None or cv["side"] == side) for m in cv["calls"]]

    def main_conv(self, run):
        cs = [cv for cv in self.convs.values() if cv["run"] is run and not cv["side"]]
        return cs[0] if cs else None

    # ---------------------------------------------------------- files (facts), tied to runs (opinions)
    def _files(self):
        cks = {l["checkpoint"]: l["at"] for l in self.control("checkpoint taken")}
        self.ck_at = cks
        rep = collections.defaultdict(list)
        for l in self.reports("file"):
            rep[(l["computer"], l["path"])].append(l)
        self.file_facts = self.facts("file")
        for f in self.file_facts:
            a, b = cks.get(f["between"][0]), cks.get(f["between"][1])
            rs = [r for r in rep.get((f["computer"], f["path"]), []) if a < r["at"] <= b]
            runs = {}
            for r in rs:
                p = self.procs.get((f["computer"], r["pid"]))
                run = p["run"] if p else None
                runs[run["id"] if run else None] = (run, r, p)
            named = [v[0] for k, v in runs.items() if k]
            if len(runs) == 1 and named:
                run, r, p = list(runs.values())[0]
                f["_by"] = {"run": run, "process": p, "report": r, "tool_call": p["tool_call"] if p else None,
                            "rests_on": f"the probe reported pid {r['pid']} {r['op']} it between the two checkpoints"}
            else:
                f["_by"] = {"run": None, "named": named, "rests_on": ("several runs reported writing it" if len(named) > 1 else
                                                                        "no report from the probe" if not rs else "the process's chain to a launch is broken")}

    # ---------------------------------------------------------- how each run is going
    def state(self, run):
        now = self.now
        alive = run["end"] is None
        calls = sorted(self.run_calls(run), key=lambda m: m["at"])
        last_call = R.ts_of(calls[-1]["at"]) if calls else R.ts_of(run["start"])
        open_tcs = [t for t in self.tool_calls if t["run"] is run and t["result_at"] is None]
        procs_alive = [p for p in self.procs.values() if p["run"] is run and p["end"] is None and p["pid"] != run["pid"] and p["tool_call"] in open_tcs]
        if not alive:
            how = "ended by itself" if run.get("exit") == 0 else (f"ended with {run['signal']}" if run.get("signal") else f"ended, exit {run.get('exit')}")
            return {"state": "ended", "how": how, "since": run["end"], "for_s": 0, "rests_on": how}
        quiet = now - last_call
        if open_tcs and procs_alive:
            cmd = procs_alive[-1]["tool_call"]["summary"]
            st = "stuck" if quiet > 60 else "waiting on a command"
            return {"state": st, "since": R.at_of(last_call), "for_s": quiet, "command": cmd, "rests_on": f"no model call for {int(quiet)} s; a command of its open tool call is alive: {cmd}"}
        if quiet < 15:
            return {"state": "working", "since": R.at_of(last_call), "for_s": quiet, "rests_on": "a model call in the last 15 s"}
        return {"state": "idle", "since": R.at_of(last_call), "for_s": quiet, "rests_on": f"no model call for {int(quiet)} s and no command alive"}

    # ---------------------------------------------------------- what a run did
    def commands(self, run):
        """The tool calls of a run that ran a command, with their result as the model was sent it
        (a fact) and the exit code the probe reported (a report)."""
        out = []
        for t in self.tool_calls:
            if t["run"] is not run or t["tool"] not in ("Bash", "exec_command", "shell"):
                continue
            res = self.text(t["result"]["text_ref"]) if t.get("result") else None
            code = None
            if res is not None:
                m = re.search(r"(?:Exit code|exit code|Process exited with code)[: ]+(-?\d+)", res)
                if m:
                    code = int(m.group(1))
                elif t["result"].get("is_error") is False:
                    code = 0
            procs = [p for p in self.procs.values() if p["tool_call"] is t]
            top = [p for p in procs if not any(q["pid"] == p["parent"] for q in procs)]
            pexit = top[0]["exit"] if top else None
            alive = [p for p in procs if p["end"] is None and not p.get("thread")]
            # what is left running: the topmost live process of each live subtree
            alive = [p for p in alive if not any(q["pid"] == p["parent"] for q in alive)]
            out.append({"tc": t, "command": t["summary"], "result_text": res, "result_exit": code, "probe_exit": pexit, "procs": procs,
                        "still_running": alive})
        return out

    def files_of(self, run):
        return [f for f in self.file_facts if f["_by"]["run"] is run]

    def spend(self, run):
        calls = self.run_calls(run)
        unknown = [m for m in calls if not m.get("price_known")]
        return {"usd": round(sum(m.get("cost_usd") or 0 for m in calls), 4), "unknown": len(unknown), "calls": len(calls),
                "models": collections.Counter(m.get("model_answered") or m.get("model_asked") for m in calls),
                "tokens_in": sum(m["tokens"]["in"] + m["tokens"]["cache_read"] + m["tokens"]["cache_write"] for m in calls),
                "cached": sum(m["tokens"]["cache_read"] for m in calls)}

    def last_words(self, run):
        cv = self.main_conv(run)
        if not cv:
            return None, None
        for m in reversed(cv["calls"]):
            if m.get("stop") == "end_turn" and m.get("reply_text_ref"):
                return self.text(m["reply_text_ref"]), m
        return None, None

    def claims(self, run):
        """claims@0.1: the claims of fixed kinds in the agent's last words, checked against the record."""
        words, m = self.last_words(run)
        if not words:
            return []
        out = []
        cmds = self.commands(run)
        tests = [c for c in cmds if TEST_CMD.search(c["command"] or "")]
        for sent in re.split(r"(?<=[.!])\s+", words.strip()):
            s = sent.lower()
            if re.search(r"\b(all )?tests? (pass|passed|are passing|green)\b", s):
                full = [c for c in tests if not FILTERED.search(c["command"])]
                last_full = full[-1] if full else None
                if not tests:
                    out.append({"claim": sent, "says": "mismatch", "rests_on": "no test command ran in this run", "line": None})
                elif last_full and (last_full["result_exit"] not in (0, None)):
                    out.append({"claim": sent, "says": "mismatch", "rests_on": f"the last run of the whole suite, `{last_full['command']}`, exited {last_full['result_exit']}", "line": last_full})
                elif tests[-1]["result_exit"] not in (0, None):
                    out.append({"claim": sent, "says": "mismatch", "rests_on": f"the last test command, `{tests[-1]['command']}`, exited {tests[-1]['result_exit']}", "line": tests[-1]})
                else:
                    out.append({"claim": sent, "says": "matched", "rests_on": f"`{tests[-1]['command']}` exited 0", "line": tests[-1]})
            elif re.search(r"only (changed|touched) files in the repo|nothing outside|did(n't| not) touch anything outside", s):
                cwd = run.get("cwd") or ""
                outside = [f for f in self.files_of(run) if not f["path"].startswith(cwd + "/") and not own_file(f["path"])]
                if outside:
                    out.append({"claim": sent, "says": "mismatch", "rests_on": f"{len(outside)} files outside {self.tilde(run['computer'], cwd)} changed by this run, first {self.tilde(run['computer'], outside[0]['path'])}", "line": outside[0]})
                else:
                    out.append({"claim": sent, "says": "matched", "rests_on": "no file outside the repository is this run's", "line": None})
            elif sent.strip():
                out.append({"claim": sent, "says": "not checked", "rests_on": "not a kind of claim claims@0.1 checks", "line": None})
        return out

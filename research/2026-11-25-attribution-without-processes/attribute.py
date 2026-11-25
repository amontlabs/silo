"""Who changed a file, and who opened a connection, from model traffic alone, measured against the probe.

    python3 attribute.py <night folder> [--coarsen N] [--json]

Option A of #373 records model traffic, file changes between checkpoints and connections, and no
processes. This reads only those facts and says, for each file fact and each connection, which
conversation it comes from and on what it rests:

  content      a tool call open in the window sent the file's whole new content (Claude Code's Write),
               and its hash is the file's hash after the change
  named        a tool call open in the fact's window names the path (or the host) in its input
  under        a tool call open in the window names a folder above the path, other than its working folder
  read named,  as named and under, from a command that only reads (ls, cat, grep...): such a command
  read under   names a path it did not change, so it counts only when nothing stronger does
  implied      a tool call open in the window runs a command known to write there: `git config
               --global` writes ~/.gitconfig, git commands write the .git folder of their repository,
               npm install writes node_modules and package-lock.json
  result       no tool call names it, and one tool call's result, the text the computer sent back to
               the model, names the path: the weakest, since whatever runs in the computer writes it
  session      an agent program's own file whose name holds the program's session identifier, which
               the request carries (Claude Code's metadata.user_id, Codex's prompt_cache_key)
  own files    the path is an agent program's own (~/.claude, ~/.codex); the program is named, and
               the conversation only when one conversation of that program was open in the window
  only one     no tool call names it, and one conversation alone had a tool call open in the window
  none         no tool call open in the window, or several conversations and none names it

It then compares each answer with what #371's probe-based classifier (which-run-did-it@0.1) said
for the same fact, mapped from run to conversation. The probe is the reference here, not the truth:
where it says "unattributed", the fact is not scored.

--coarsen N keeps one checkpoint in N, to see what a checkpoint every 5 minutes (#370) does to a
night that is compressed to 15 s between checkpoints.
"""
import collections, hashlib, json, os, re, shlex, sys

HERE = os.path.dirname(os.path.abspath(__file__))
PROTO = os.path.join(HERE, "..", "2026-11-24-morning-review", "proto")
sys.path.insert(0, PROTO)
sys.dont_write_bytecode = True
import record as R      # noqa: E402
import classify as C    # noqa: E402


class View(C.View):
    """#371's classifier, which matches tool calls to processes for shell tools only. Code mode's
    `exec` runs commands too, so the reference reads it as one (the summary is its command)."""
    def facts(self, line=None, computer=None):
        out = super().facts(line, computer)
        if line == "model call":
            for m in out:
                for tc in m.get("tool_calls") or []:
                    if tc["tool"] == "exec":
                        tc["tool"], tc["code_mode"] = "exec_command", True
        return out

READ_ONLY = {"ls", "cat", "head", "tail", "wc", "grep", "rg", "find", "stat", "du", "file", "less", "diff", "echo", "printf", "test", "realpath", "readlink"}
SHELL_TOOLS = ("Bash", "exec_command", "shell", "exec")
PATH_KEYS = ("file_path", "path", "notebook_path")


def opt(name, default):
    return type(default)(sys.argv[sys.argv.index(name) + 1]) if name in sys.argv else default


# ---------------------------------------------------------------- reading the traffic

def commands_of(tool, inp):
    """The shell command lines a tool call asks for, and the folder each runs in, if it says.
    Codex's code mode sends JavaScript: the commands are the string literals passed to exec_command."""
    if tool == "exec" and isinstance(inp, str):
        out = []
        for m in re.finditer(r"exec_command\(\s*\{([^}]*)\}", inp):
            body = m.group(1)
            cmd = re.search(r"cmd\s*:\s*(\"(?:[^\"\\]|\\.)*\"|'(?:[^'\\]|\\.)*'|`[^`]*`)", body)
            wd = re.search(r"workdir\s*:\s*(\"(?:[^\"\\]|\\.)*\")", body)
            if cmd:
                lit = cmd.group(1)
                try:
                    s = json.loads(lit) if lit[0] == '"' else lit[1:-1]
                except Exception:
                    s = lit[1:-1]
                out.append((s, json.loads(wd.group(1)) if wd else None))
        # also any other string literal, for paths the script builds or writes itself
        return out
    if not isinstance(inp, dict):
        return []
    for k in ("command", "cmd"):
        if k in inp:
            v = inp[k]
            if isinstance(v, list):
                v = v[-1] if v[:2] == ["bash", "-lc"] else " ".join(v)
            return [(v, inp.get("workdir"))]
    return []


def paths_of(tool, inp):
    if isinstance(inp, dict):
        return [inp[k] for k in PATH_KEYS if isinstance(inp.get(k), str)]
    if isinstance(inp, str) and tool in ("apply_patch", "exec"):
        return re.findall(r"^\*\*\* (?:Add|Update|Delete) File: (.+)$", inp, re.M)
    return []


def words(cmd):
    try:
        return shlex.split(cmd, posix=True)
    except ValueError:
        return cmd.split()


def split_cd(cmd, wd):
    """`cd X && rest` -> (X, rest). The folder a command runs in is not one of its operands."""
    m = re.match(r"\s*cd\s+(\S+)\s*(?:&&|;)\s*(.*)$", cmd, re.S)
    if m:
        return m.group(1), m.group(2)
    return wd, cmd


def resolve(tok, cwd, home):
    tok = tok.strip("\"'")
    if tok.startswith("~/"):
        return os.path.normpath(home + tok[1:])
    if tok.startswith("/"):
        return os.path.normpath(tok)
    if cwd and re.match(r"^[\w.][\w./*-]*$", tok) and not tok.startswith("-"):
        return os.path.normpath(os.path.join(cwd, tok))
    return None


class Call:
    def __init__(self, t, home):
        if t.get("code_mode"):
            t["tool"] = "exec"
        self.id, self.tool, self.conversation, self.computer = t["id"], t["tool"], t["conversation"], t["computer"]
        self.start, self.end = R.ts_of(t["asked_at"]), (R.ts_of(t["result_at"]) if t["result_at"] else None)
        self.input = t["input"]
        self.result = t.get("result_text")
        self.cwd, self.operands, self.text, self.cds, self.read_operands = None, set(), "", set(), set()
        for cmd, wd in commands_of(self.tool, self.input):
            cwd, rest = split_cd(cmd, wd)
            cwd = resolve(cwd, None, home) if cwd else None
            self.cwd = self.cwd or cwd
            self.text += " " + rest
            # a folder a later `cd` moves into is where the command runs, not what it works on
            for m in re.finditer(r"(?:^|[;&|(]\s*)cd\s+(\S+)", rest):
                d = resolve(m.group(1), cwd, home)
                self.cwd = self.cwd or d
                self.cds.add(d)
            rest_ops = re.sub(r"(?:^|(?<=[;&|(]))\s*cd\s+\S+", " ", rest)
            for seg in re.split(r"&&|\|\||[;|()]", rest_ops):
                for m in re.finditer(r"(?<![0-9&])>>?\s*([^\s;&|]+)", seg):  # a redirection writes its target
                    p = resolve(m.group(1), cwd, home)
                    if p and not p.startswith("/dev/"):
                        self.operands.add(p)
                seg = re.sub(r"[0-9&]?>>?\s*[^\s;&|]+", " ", seg)
                ws = [w for w in re.split(r"<+|\s+", seg) if w]
                reads = bool(ws) and os.path.basename(ws[0]) in READ_ONLY and "-delete" not in ws
                for w in ws:
                    for part in [w] + w.split("="):
                        p = resolve(part, cwd, home)
                        if p and p != home and not home.startswith(p + "/"):  # the home folder or above it says nothing
                            (self.read_operands if reads else self.operands).add(p.rstrip("*").rstrip("/") if "*" not in p else p)
        for p in paths_of(self.tool, self.input):
            p = resolve(p, self.cwd, home)
            if p:
                self.operands.add(p)
        if isinstance(self.input, str):
            self.text += " " + self.input

    def open_in(self, a, b, night_end):
        return self.start <= b and (self.end or night_end) >= a

    def wrote_content(self, sha):
        """Claude Code's Write sends the whole file: its hash is the file's after the change."""
        if not sha or not isinstance(self.input, dict) or not isinstance(self.input.get("content"), str):
            return False
        return hashlib.sha256(self.input["content"].encode()).hexdigest() == sha

    def implied(self, path, home):
        t = self.text
        if re.search(r"\bgit\s+config\s+--global\b", t) and path == home + "/.gitconfig":
            return "implied"
        if self.cwd and re.search(r"\bgit\s+(add|commit|checkout|switch|reset|merge|rebase|stash|tag|branch|fetch|pull|gc|restore|rm|mv)\b", t):
            d = self.cwd
            while d and d != home and d != "/":
                if path.startswith(d + "/.git/"):
                    return "implied"
                d = os.path.dirname(d)
        if self.cwd and re.search(r"\b(npm|pnpm|yarn|bun)\s+(install|i|add|ci)\b", t) and (
                path.startswith(self.cwd + "/node_modules/") or path in (self.cwd + "/package-lock.json", self.cwd + "/bun.lockb", self.cwd + "/bun.lock")):
            return "implied"
        return None

    def match(self, path, reads=False):
        """named or under, from the operands of commands that may write; with reads=True, from
        the operands of commands that only read (ls, cat, grep...), a weaker sign."""
        ops = self.read_operands if reads else self.operands
        for o in ops:
            if "*" in o:
                if re.fullmatch(re.escape(o).replace(r"\*", "[^/]*"), path):
                    return "named"
            elif o == path:
                return "named"
        for o in ops:
            if "*" not in o and path.startswith(o + "/") and o != self.cwd and o not in self.cds:
                return "under"
        return None


def tool_calls(view, home_of):
    """Tool calls from the model-call facts alone: id, input, window, result."""
    out = []
    for cv in view.convs.values():
        calls = cv["calls"]
        for i, m in enumerate(calls):
            for tc in m.get("tool_calls") or []:
                end, res = None, None
                for n in calls[i + 1:]:
                    for rs in n.get("results_sent") or []:
                        if rs["tool_call"] == tc["id"]:
                            end, res = n["started"], rs
                    if end:
                        break
                raw = R.text(view.root, tc.get("input_ref"))
                try:
                    inp = json.loads(raw) if raw is not None else None
                except Exception:
                    inp = raw
                t = {"id": tc["id"], "tool": tc["tool"], "code_mode": tc.get("code_mode"), "conversation": cv["id"], "computer": m["computer"], "asked_at": m["at"],
                     "result_at": end, "input": inp, "result_text": R.text(view.root, res["text_ref"]) if res else None,
                     "result_error": res.get("is_error") if res else None}
                out.append(Call(t, home_of[m["computer"]]))
    return out


def program_of_conversation(cv):
    m = cv["calls"][0]
    return "claude" if m.get("provider") == "anthropic" else "codex" if m.get("provider") == "openai" else None


def session_id(cv):
    """The program's session identifier, if the boundary kept it (patch_proto.py --fix)."""
    s = cv["calls"][0].get("session")
    if not s:
        return None
    try:
        return json.loads(s).get("session_id")
    except Exception:
        return s


# hosts an agent program reaches for itself, a data list like the one that recognises programs
OWN_HOSTS = {"codex": {"chatgpt.com", "ab.chatgpt.com"}, "claude": {"statsig.anthropic.com", "sentry.io"}}

OWN = {"claude": re.compile(r"/\.claude/|/\.claude\.json"), "codex": re.compile(r"/\.codex/")}


# ---------------------------------------------------------------- attribution

def attribute(view, coarsen=1):
    home_of = view.homes
    calls = tool_calls(view, home_of)
    progs = {cv["id"]: program_of_conversation(cv) for cv in view.convs.values()}
    sessions = {cv["id"]: session_id(cv) for cv in view.convs.values()}
    conv_span = {cv["id"]: (R.ts_of(cv["calls"][0]["started"]), R.ts_of(cv["calls"][-1]["at"])) for cv in view.convs.values()}
    night_end = max(R.ts_of(l["at"]) for l in view.lines)
    cks = {}
    for l in view.control("checkpoint taken"):
        cks.setdefault(l["computer"], []).append((R.ts_of(l["at"]), l["checkpoint"]))
    ck_at = {cid: t for c in cks.values() for t, cid in c}
    coarse = {}
    for c, lst in cks.items():
        lst.sort()
        kept = [t for k, (t, _) in enumerate(lst) if k % coarsen == 0] + [lst[-1][0]]
        coarse[c] = sorted(set(kept))

    def window(f):
        a, b = ck_at[f["between"][0]], ck_at[f["between"][1]]
        if coarsen > 1:
            ks = coarse[f["computer"]]
            a = max([t for t in ks if t <= a] or [a])
            b = min([t for t in ks if t >= b] or [b])
        return a, b

    results = []
    # with fewer checkpoints, the facts of one path in one wider window are one fact; the probe's
    # answer for it is its runs' answer when they all agree, and unattributed when they do not
    groups = collections.OrderedDict()
    for f in view.file_facts:
        a, b = window(f)
        groups.setdefault((f["computer"], f["path"], a, b), []).append(f)
    windows = collections.Counter()
    for (comp, path, a, b), fs in groups.items():
        f = fs[-1]
        windows[(comp, a, b)] += 0
        cands = [t for t in calls if t.computer == f["computer"] and t.open_in(a, b, night_end)]
        prog_own = next((p for p, rx in OWN.items() if rx.search(f["path"]) and not f["path"].endswith(("CLAUDE.md", "AGENTS.md"))), None)
        by, how, tc = None, "none", None
        levels = collections.defaultdict(set)
        for t in cands:
            if f["change"] != "deleted" and t.wrote_content(f.get("sha_after")):
                levels["content"].add((t.conversation, t.id))
        for t in cands:
            lv = t.match(f["path"])
            if lv:
                levels[lv].add((t.conversation, t.id))
        for t in cands:
            if not t.match(f["path"]) and t.implied(f["path"], home_of[f["computer"]]):
                levels["implied"].add((t.conversation, t.id))
        for t in cands:
            lv = t.match(f["path"], reads=True)
            if lv:
                levels["read " + lv].add((t.conversation, t.id))
        for lv in ("content", "named", "under", "implied", "read named", "read under"):
            if levels[lv]:
                convs = {c for c, _ in levels[lv]}
                if len(convs) == 1:
                    by, how = convs.pop(), lv
                    ids = [i for _, i in levels[lv]]
                    tc = ids[0] if len(ids) == 1 else None
                else:
                    how = lv + ", several conversations"
                break
        if by is None and how == "none" and not prog_own:
            home = home_of[f["computer"]]
            short = "~" + f["path"][len(home):] if f["path"].startswith(home) else f["path"]
            said = {(t.conversation, t.id) for t in cands if t.result and (f["path"] in t.result or short in t.result)}
            if len({c for c, _ in said}) == 1:
                by, how = next(iter(said))[0], "result"
                tc = next(iter(said))[1] if len(said) == 1 else None
        if by is None and how == "none" and prog_own:
            hit = {c for c, sid in sessions.items() if sid and c.split(":")[0] == f["computer"] and sid in f["path"]}
            if len(hit) == 1:
                by, how = hit.pop(), "session"
        if by is None and how == "none" and prog_own:
            # the program's own files: conversations of that program alive in the window
            live = {c for c, (s, e) in conv_span.items() if progs[c] == prog_own and c.split(":")[0] == f["computer"] and s <= b and e + 5 >= a}
            how = "own files"
            by = live.pop() if len(live) == 1 else None
        if by is None and how == "none":
            convs = {t.conversation for t in cands}
            if len(convs) == 1:
                by, how = convs.pop(), "only one"
                tc = cands[0].id if len(cands) == 1 else None
        refs, ref_tcs = set(), set()
        for g in fs:
            ref = g["_by"]
            rc = [cv["id"] for cv in view.convs.values() if ref.get("run") and cv["run"] is ref["run"] and not cv["side"]]
            refs.add(rc[0] if rc else None)
            ref_tcs.add(ref.get("tool_call")["id"] if ref.get("tool_call") else None)
        ref_conv = refs.pop() if len(refs) == 1 else None
        ref_tc = ref_tcs.pop() if len(ref_tcs) == 1 else None
        results.append({"kind": "file", "id": f["id"], "path": view.tilde(f["computer"], f["path"]), "change": f["change"], "how": how, "by": by, "tool_call": tc,
                        "ref": ref_conv, "ref_tool_call": ref_tc, "own": bool(prog_own), "program_named": prog_own})

    per_window = collections.Counter()
    for (comp, a, b) in windows:
        per_window[(comp, a, b)] = len({t.id for t in calls if t.computer == comp and t.open_in(a, b, night_end)})
    attribute.calls_per_window = (sum(per_window.values()) / len(per_window)) if per_window else 0
    owner = view.owner
    for l in view.facts("connection"):
        at = R.ts_of(l.get("started") or l["at"])
        cands = [t for t in calls if t.computer == l["computer"] and t.start <= at <= (t.end or night_end)]
        named = {(t.conversation, t.id) for t in cands if l["host"] in t.text}
        by, how, tc = None, "none", None
        if named:
            convs = {c for c, _ in named}
            if len(convs) == 1:
                by, how = convs.pop(), "named"
                tc = next(iter(named))[1] if len(named) == 1 else None
            else:
                how = "named, several conversations"
        elif len({t.conversation for t in cands}) == 1:
            by, how = cands[0].conversation, "only one"
        else:
            prog = next((p for p, hosts in OWN_HOSTS.items() if l["host"] in hosts), None)
            if prog:
                live = {c for c, (s0, e) in conv_span.items() if progs[c] == prog and c.split(":")[0] == l["computer"] and s0 - 30 <= at <= e + 5}
                how = "own host"
                by = live.pop() if len(live) == 1 else None
        pid = owner.get(l["id"])
        proc = view.procs.get((l["computer"], pid)) if pid else None
        run = proc["run"] if proc else None
        rc = [cv["id"] for cv in view.convs.values() if run and cv["run"] is run and not cv["side"]]
        ref_tc = proc["tool_call"]["id"] if proc and proc.get("tool_call") else None
        by_program = bool(proc and run and proc["pid"] == run["pid"])
        results.append({"kind": "connection", "id": l["id"], "path": l["host"], "change": l["decision"], "how": how, "by": by, "tool_call": tc,
                        "ref": rc[0] if rc else None, "ref_tool_call": ref_tc, "own": by_program, "program_named": None})
    return results, calls


def score(results):
    rows = collections.OrderedDict()
    for r in results:
        key = (r["kind"], "agent program's own" if r["own"] else "the work")
        s = rows.setdefault(key, collections.Counter())
        s["n"] += 1
        if r["ref"] is None:
            s["probe: unattributed"] += 1
            continue
        s["scored"] += 1
        if r["by"] is None:
            s["none"] += 1
        elif r["by"] == r["ref"]:
            s["right"] += 1
            if r["how"] in ("content", "named", "under", "implied"):
                s["right, named or under"] += 1
            if r["tool_call"] and r["tool_call"] == r["ref_tool_call"]:
                s["right tool call"] += 1
        else:
            s["wrong"] += 1
    return rows


def main():
    night = os.path.abspath(sys.argv[1])
    view = View(os.path.join(night, "record"), now=None)
    coarsen = opt("--coarsen", 1)
    results, calls = attribute(view, coarsen)
    if "--json" in sys.argv:
        json.dump(results, sys.stdout, indent=1)
        return
    print(f"night {os.path.basename(night)} · {len(calls)} tool calls · checkpoints: one in {coarsen} · tool calls open in a window with a file fact: {attribute.calls_per_window:.1f} on average")
    hdr = ["facts", "n", "probe: unattributed", "scored", "right", "right, named or under", "right tool call", "wrong", "none"]
    print(f"{hdr[0]:<38}" + "".join(f"{h:>14}" for h in ["n", "unattr. (probe)", "scored", "right", "named/under", "right call", "wrong", "none"]))
    for (kind, who), s in score(results).items():
        print(f"{kind + ', ' + who:<38}" + "".join(f"{s[h]:>14}" for h in hdr[1:]))
    hows = collections.Counter((r["kind"], r["how"]) for r in results if not r["own"])
    print("how, the work:", dict(hows))
    if "--wrong" in sys.argv:
        for r in results:
            if r["ref"] and r["by"] and r["by"] != r["ref"]:
                print("WRONG", r)
            if "--none" in sys.argv and r["ref"] and not r["by"]:
                print("NONE ", r["kind"], r["path"], r["how"], "probe:", r["ref"])


if __name__ == "__main__":
    main()

"""A night of #371's prototype read as option A of #373 would read it: the model traffic, the file
facts between two checkpoints and the connections. No process and no report of the probe is read
by anything this module returns.

The checkpoints are #371's: a manifest per checkpoint (`record/<computer>/checkpoints/ckN.json`)
and each file's content once by its hash (`record/cas/<sha256>`). They stand for the APFS clones of
#370 step 4; the disk reader of step 5 stands behind `manifest()` and `content()`.

Authors come from #399's `attribute.py`, kept as it is. This module adds one thing to it: which of
its rules are strong enough to draw a link in a chain (see STRONG).
"""
import collections, datetime, json, os, re, sys

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "2026-11-25-attribution-without-processes"))
sys.argv = sys.argv[:1] + [a for a in sys.argv[1:]]  # attribute.py reads sys.argv only in main()
import attribute as A   # noqa: E402
import commands as K    # noqa: E402

R = A.R

# The rules `author-from-traffic` answers with: what one tool call said (its content, a path, a
# folder above, a command known to write there) and the program's session identifier in a file's
# name. On a second run of #399's `one-main` night these stayed at 0 wrong, while the rules that
# entry leaves out, which rest on timing ("own files", "only one"), named a wrong conversation 3
# times (results/attribution-rerun.txt). A chain draws a link only on these.
STRONG = ("content", "named", "under", "implied", "session")
RULE_WORDS = {
    "content": "the tool call sent the file's whole new content",
    "named": "the tool call names this path",
    "under": "the tool call names a folder above this path",
    "implied": "the tool call runs a command known to write here",
    "read named": "the tool call names this path, in a command that only reads",
    "read under": "the tool call names a folder above, in a command that only reads",
    "result": "a tool result names this path",
    "session": "the file's name holds the program's session identifier",
    "own files": "an agent program's own file, one conversation of that program was running",
    "only one": "one conversation alone had a tool call open",
}


class Night:
    def __init__(self, folder):
        self.folder = os.path.abspath(folder)
        self.root = os.path.join(self.folder, "record")
        self.view = A.View(self.root, now=None)
        self.results, self.calls = A.attribute(self.view)
        self.attr = {r["id"]: r for r in self.results if r["kind"] == "file"}
        self.call = {c.id: c for c in self.calls}
        self.model_call_of = {}       # tool call id -> the model call fact that asked for it
        for cv in self.view.convs.values():
            for m in cv["calls"]:
                for tc in m.get("tool_calls") or []:
                    self.model_call_of[tc["id"]] = m
        self.homes = self.view.homes

    # ------------------------------------------------------------ facts only
    def facts(self, computer, line=None):
        out = []
        p = os.path.join(self.root, computer, "facts.jsonl")
        for raw in open(p):
            if raw.strip():
                o = json.loads(raw)
                if line is None or o["line"] == line:
                    out.append(o)
        return out

    def checkpoints(self, computer):
        """[(time, 'main@ckN', N, why)], oldest first."""
        out = []
        for l in self.facts(computer, "control"):
            if l.get("what") == "checkpoint taken":
                out.append((R.ts_of(l["at"]), l["checkpoint"], int(l["checkpoint"].split("@ck")[1]), l.get("why")))
        return sorted(out)

    def manifest(self, computer, n):
        return json.load(open(os.path.join(self.root, computer, "checkpoints", f"ck{n}.json")))

    def content(self, sha):
        p = os.path.join(self.root, "cas", sha)
        return p if os.path.exists(p) else None

    def home(self, computer):
        return self.homes[computer]

    def tilde(self, computer, p):
        """A path for the screen: ~ for the home folder, control characters escaped."""
        h = self.home(computer)
        return safe("~" + p[len(h):] if p == h or p.startswith(h + "/") else p)

    def untilde(self, computer, p):
        p = p.rstrip("/") or "/"
        return os.path.normpath(self.home(computer) + p[1:]) if p.startswith("~") else os.path.normpath(p)

    # ------------------------------------------------------------ time
    def resolve_time(self, computer, s):
        """A checkpoint (`ck12`, `main@ck12`), a fact id (the checkpoint before that change), or a
        time of day (`14:16:25`, local to the record's offset): the newest checkpoint at or before
        it. Returns (time, id, N) or raises ValueError with the sentence to print."""
        cks = self.checkpoints(computer)
        if not cks:
            raise ValueError(f"{computer} has no checkpoint")
        m = re.fullmatch(r"(?:\w+@)?ck(\d+)", s)
        if m:
            n = int(m.group(1))
            for t, cid, k, _ in cks:
                if k == n:
                    return t, cid, k
            raise ValueError(f"{computer} has no checkpoint ck{n}")
        if re.fullmatch(r"\w[\w-]*:f\d+", s):
            f = next((x for x in self.facts(computer) if x["id"] == s), None)
            if not f or f["line"] != "file":
                raise ValueError(f"{s} is not a file change of {computer}")
            n = int(f["between"][0].split("@ck")[1])
            return next((t, cid, k) for t, cid, k, _ in cks if k == n)
        m = re.fullmatch(r"(\d\d):(\d\d)(?::(\d\d)(?:\.(\d+))?)?", s)
        if not m:
            raise ValueError(f"not a time: {s} (give a time of day, a checkpoint such as ck12, or a change such as main:f479)")
        first = datetime.datetime.fromtimestamp(cks[0][0], datetime.datetime.fromisoformat(self.facts(computer, "control")[0]["at"]).tzinfo)
        want = first.replace(hour=int(m.group(1)), minute=int(m.group(2)), second=int(m.group(3) or 0),
                             microsecond=int((m.group(4) or "0").ljust(6, "0")[:6]) if m.group(4) else (999999 if not m.group(3) else 999999)).timestamp()
        before = [(t, cid, k) for t, cid, k, _ in cks if t <= want]
        if not before:
            raise ValueError(f"no checkpoint of {computer} at or before {s}: the first is at {hm(cks[0][0])}")
        return before[-1]

    # ------------------------------------------------------------ authors under A
    def author(self, fact_id):
        """#399's answer for one file fact: {how, by, tool_call, strong}."""
        r = self.attr.get(fact_id)
        if not r:
            return {"how": "none", "by": None, "tool_call": None, "strong": False, "candidates": []}
        out = {"how": r["how"], "by": r["by"], "tool_call": r["tool_call"], "strong": r["how"] in STRONG}
        out["candidates"] = self.open_calls(fact_id)
        return out

    def open_calls(self, fact_id):
        """The tool calls open in the window of a file fact, and the conversations they belong to."""
        f = self.fact(fact_id)
        ck = {cid: t for t, cid, _, _ in self.checkpoints(f["computer"])}
        a, b = ck[f["between"][0]], ck[f["between"][1]]
        end = max(R.ts_of(l["at"]) for l in self.view.lines)
        return [c for c in self.calls if c.computer == f["computer"] and c.open_in(a, b, end)]

    def fact(self, fact_id):
        c = fact_id.split(":")[0]
        if not hasattr(self, "_by_id"):
            self._by_id = {}
        if c not in self._by_id:
            self._by_id[c] = {x["id"]: x for x in self.facts(c)}
        return self._by_id[c].get(fact_id)

    def conversation(self, cid):
        return self.view.convs[cid]

    def program(self, cid):
        return A.program_of_conversation(self.view.convs[cid])

    def exit_of(self, call):
        """The exit code the traffic carries for a tool call, as #399's commands.py reads it."""
        if call.result is None:
            return "no result: still running"
        for rx in K.EXIT:
            m = rx.search(call.result)
            if m:
                return f"exit {m.group(1)}"
        if any(rx.search(call.result) for rx in K.RUNNING):
            return "still running"
        return "no error"

    def summary(self, call):
        """A tool call's input in one line, with the home folder written ~."""
        h = self.home(call.computer)
        return self._summary(call).replace(h + "/", "~/").replace(h, "~")

    def _summary(self, call):
        cmds = A.commands_of(call.tool, call.input)
        if cmds:
            return " ; ".join(c for c, _ in cmds)
        if isinstance(call.input, dict):
            p = next((call.input[k] for k in A.PATH_KEYS if isinstance(call.input.get(k), str)), None)
            if p:
                return p
            return json.dumps(call.input)[:200]
        return str(call.input)[:200]

    def label(self, cid):
        if not cid:
            return "no conversation"
        n = sorted(c for c in self.view.convs if self.program(c) == self.program(cid)).index(cid) + 1
        return f"{self.program(cid)} #{n} ({cid})"


def hm(ts):
    return datetime.datetime.fromtimestamp(ts).astimezone().strftime("%H:%M:%S")


def safe(text):
    """Control characters escaped, so a name an agent chose cannot drive the terminal (#370 step 5)."""
    return "".join(ch if ch >= " " and ch != "\x7f" else f"\\x{ord(ch):02x}" for ch in str(text))


def mark(text):
    """Text an agent wrote, printed marked and escaped."""
    return f"«{safe(text)}»"


def words(text, n):
    s = " ".join(str(text).split())
    return s if len(s) <= n else s[:n - 1] + "…"

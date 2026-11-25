"""The agent's last words, checked against the record. Two versions, side by side:

  claims@0.1  two kinds: "tests pass" and "only changed files in the repo" (the night of #371)
  claims@0.2  more kinds, written before reading the corpus of public incidents in
              research/2026-11-25-real-night, and a fourth answer, "unbacked": the record holds no
              fact that shows the claim, though none contradicts it (a piped run, a narrowed run,
              an edit after the last run)
  claims@0.3  0.2 plus what the corpus's development half taught: negated pushes, "stopped" and
              "killed", "fixed" and "works", a named file that is "ready" or "copied". Its catch rate
              is read on the other half only

Both read the same evidence, built from the record by classify.py or from a corpus case:

  commands      [{command, exit, t}]       exit as the model was sent it (None: not known);
                                           t orders it against the edits
  edits         [{path, change, inside, t}] files this run changed, not the program's own; inside:
                                           in the run's folder; t: the earliest it can have happened
  left_running  [command]                  processes of this run still alive at its end
  connections   [{host, decision, method}] connections of this run other than model calls
  cwd           the run's folder

Each claim gets {claim, kind, says: matched|mismatch|unbacked|not checked, rests_on, line}.
"""
import re

TEST_CMD = re.compile(r"(^|[\s;&|(])(node\s+--test|npm\s+(run\s+)?test|npx\s+(jest|vitest|playwright)|yarn\s+test|pnpm\s+(run\s+)?test|pytest|python3?\s+-m\s+(pytest|unittest)|bun\s+test|cargo\s+(test|nextest)|go\s+test|jest|vitest|rspec|phpunit|mvn\s+test|gradlew?\s+test|xcodebuild\s+test|swift\s+test|make\s+test|/bun\s+test)(\s|$)")
FILTERED = re.compile(r"(--test|test|pytest|jest|vitest)\s+(?!--)\S*[/.*]|\s-k\s|--grep|-t\s|--testNamePattern|::")
BUILD_CMD = re.compile(r"(^|[\s;&|(])(tsc|npm\s+run\s+(build|lint|typecheck)|yarn\s+build|pnpm\s+(run\s+)?build|cargo\s+(build|check|clippy)|go\s+(build|vet)|make(\s|$)|xcodebuild|swift\s+build|mvn\s+(compile|package)|gradlew?\s+build|eslint|ruff|mypy|next\s+build|vite\s+build)")
# Flags that narrow a run to some tests, and flags that take a value without narrowing it
NARROWING_FLAGS = {"-k", "-t", "-g", "-m", "--grep", "--testNamePattern", "--test-name-pattern", "-run", "--run", "--test", "--filter", "--only", "--lf", "--last-failed", "--testPathPattern", "--project", "--spec", "-e", "--example", "--tests", "--package", "-p"}
VALUE_FLAGS = {"--reporter", "-c", "--config", "--workers", "-j", "--jobs", "-n", "--maxWorkers", "--timeout", "--color", "--profile", "--features", "-r", "--require", "--runInBand", "--pool"}
WHOLE_ARGS = {".", "./", "./...", "...", "test", "test/", "tests", "tests/", "spec", "spec/", "__tests__", "__tests__/", "run"}


def narrowed(command):
    """True when a test command runs only part of the suite: it names files, folders, tests or a
    pattern after the runner (`npm test -- test/auth`, `pytest -k auth`, `cargo test auth`,
    `go test ./pkg/auth`). A folder that is the usual home of the whole suite (`tests/`, `./...`)
    does not narrow it. #382 found `npm test -- test/auth` read as the whole suite."""
    command = command or ""
    if "::" in command:
        return True
    m = TEST_CMD.search(command)
    if not m:
        return bool(FILTERED.search(command))
    rest = re.split(r"\s(?:\||;|&&|\|\||\d?>)", " " + command[m.end(2):] + " ")[0]
    tokens = rest.split()
    i = 0
    while i < len(tokens):
        tok = tokens[i]
        if tok == "--":
            i += 1
            continue
        if tok.startswith("-"):
            flag = tok.split("=", 1)[0]
            if flag in NARROWING_FLAGS:
                return True
            if flag in VALUE_FLAGS and "=" not in tok:
                i += 1
            i += 1
            continue
        if tok not in WHOLE_ARGS:
            return True
        i += 1
    return False


PIPED = re.compile(r"\|\s*(tail|head|grep|less|cut|sed|awk)\b")
TEST_PATH = re.compile(r"(^|/)(tests?|__tests__|spec|specs)/|[._-](test|spec)\.[a-z]+$|_test\.(go|py)$|(^|/)test_[^/]*\.py$|conftest\.py$|(^|/)(jest|vitest|pytest)\.config|(^|/)Makefile$|\.github/workflows/")
OWN = re.compile(r"/\.(claude|codex|gemini)/|/\.claude\.json")


def sentences(words):
    parts = re.split(r"(?<=[.!?])\s+|\n+\s*(?:[-*•]|\d+\.)?\s*", (words or "").strip())
    return [p.strip() for p in parts if p and p.strip(" -*•")]


def _last(xs):
    return xs[-1] if xs else None


# ------------------------------------------------------------------ claims@0.1

def check_01(words, ev):
    out = []
    tests = [c for c in ev["commands"] if TEST_CMD.search(c["command"] or "")]
    for sent in re.split(r"(?<=[.!])\s+", (words or "").strip()):
        s = sent.lower()
        if re.search(r"\b(all )?tests? (pass|passed|are passing|green)\b", s):
            full = [c for c in tests if not narrowed(c["command"])]
            last_full = _last(full)
            if not tests:
                out.append(_c(sent, "tests-pass", "mismatch", "no test command ran in this run"))
            elif last_full and last_full["exit"] not in (0, None):
                out.append(_c(sent, "tests-pass", "mismatch", f"the last run of the whole suite, `{last_full['command']}`, exited {last_full['exit']}", last_full))
            elif tests[-1]["exit"] not in (0, None):
                out.append(_c(sent, "tests-pass", "mismatch", f"the last test command, `{tests[-1]['command']}`, exited {tests[-1]['exit']}", tests[-1]))
            else:
                out.append(_c(sent, "tests-pass", "matched", f"`{tests[-1]['command']}` exited 0", tests[-1]))
        elif re.search(r"only (changed|touched) files in the repo|nothing outside|did(n't| not) touch anything outside", s):
            outside = [f for f in ev["edits"] if not f["inside"]]
            if outside:
                out.append(_c(sent, "scope", "mismatch", f"{len(outside)} files outside {ev['cwd']} changed by this run, first {outside[0]['path']}", outside[0]))
            else:
                out.append(_c(sent, "scope", "matched", "no file outside the run's folder is this run's"))
        elif sent.strip():
            out.append(_c(sent, None, "not checked", "not a kind of claim claims@0.1 checks"))
    return out


def _c(claim, kind, says, rests_on, line=None):
    return {"claim": claim, "kind": kind, "says": says, "rests_on": rests_on, "line": line}


# ------------------------------------------------------------------ claims@0.2

NEG = r"(did ?n[o']t|didn't|have ?n[o']t|haven't|never|no|not|without|nothing|none of)"

KINDS = [
    # (kind, pattern on the lowercased sentence); the first that matches decides
    ("tests-unchanged", rf"\b{NEG}\b[^.]*\b(chang|modif|touch|edit|weaken|skip|disabl|delet)\w*\b[^.]*\btests?\b|\btests?\b[^.]*\b(were|was)\s+(not|n't)\s+(chang|modif|touch|edit|skip)|\bwithout (changing|modifying|touching|skipping) (any )?(of )?(the )?tests?"),
    ("scope", r"only (changed|touched|modified|edited) (files|code)? ?(in|inside|within) (the )?(repo|project|folder|directory)|nothing outside|did ?n[o']t (touch|change|modify) anything (outside|else)|no (other )?files? outside|outside the (repo|project) (was|were) (not )?(touched|changed)|only (changed|modified|touched|edited) [\w./-]+\.[a-z]+"),
    ("nothing-deleted", rf"\b{NEG}\b[^.]*\b(delet|remov|rm\b|wip|destroy|lost)\w*|\bnothing (was|got) (deleted|removed|lost)|\bmoved\b[^.]*\b(not|instead of) delet"),
    ("tests-pass", r"\b(all|every|the)? ?(\d+ ?/ ?\d+ |\d+ )?(unit |integration |e2e )?tests?( suite)? (now )?(pass|passed|passes|are passing|is passing|succeed|succeeded|are green|is green|run green|all pass)|\b(all|\d+) (\w+ )?(tests|specs|checks) (pass|passed|green)|\btest suite (passes|is green|passed)|\ball green\b|\b\d+ passed\b|\bci (is )?(green|passes|passing)\b|\bno (test )?failures\b|\b0 fail"),
    ("build-ok", r"\b(build|compil\w*|typecheck\w*|type-check\w*|lint\w*)\b[^.]*\b(succeed\w*|pass\w*|clean|works|ok|green|without (errors|warnings))|\bno (type|lint|compile|build) errors\b|\bcompiles( cleanly)?\b|\bbuilds (cleanly|successfully)\b"),
    ("left-running", rf"\b(stopped|killed|shut down|terminated|cleaned up)\b[^.]*\b(server|process|daemon|watcher|background|job)|\b{NEG}\b[^.]*\b(process|server|daemon)\w*\b[^.]*\b(running|left|alive)|\bnothing (is )?(left )?running\b"),
    ("is-running", r"\b(server|daemon|service|app) (is|was) (now )?(running|up|started|listening)|\b(started|launched) the (server|daemon|service)"),
    ("pushed", r"\b(pushed|opened (a |the )?(pr|pull request)|created (a |the )?(pr|pull request)|deployed|merged)\b"),
    ("committed", r"\bcommitted\b|\bcreated (a |the )?commit\b|\bchanges are committed\b"),
    ("nothing-sent", rf"\b{NEG}\b[^.]*\b(sent|send|upload|post|shar|leak)\w*\b|\bnothing (was )?(sent|uploaded|shared)\b|\bstays? (local|on (your|the) machine)"),
    ("verified", r"\b(verified|confirmed|double[- ]checked|tested it|validated|i ran|ran the|checked that|works now|is working|now works|fixed)\b"),
]


def check_02(words, ev, kinds=None):
    cmds = ev["commands"]
    edits = [e for e in ev["edits"] if not OWN.search(e["path"])]
    tests = [c for c in cmds if TEST_CMD.search(c["command"] or "")]
    builds = [c for c in cmds if BUILD_CMD.search(c["command"] or "")]
    code_edits = [e for e in edits if e["inside"] and not TEST_PATH.search(e["path"])]
    test_edits = [e for e in edits if TEST_PATH.search(e["path"])]
    out = []
    for sent in sentences(words):
        s = sent.lower()
        kind = next((k for k, p in (kinds or KINDS) if re.search(p, s)), None)
        if kind is None:
            out.append(_c(sent, None, "not checked", f"not a kind of claim claims@{'0.3' if kinds else '0.2'} checks"))
            continue
        out.append(globals()["_" + kind.replace("-", "_")](sent, kind, s, ev, cmds, edits, tests, builds, code_edits, test_edits))
    return out


def _after(e, c):
    return e.get("t") is not None and c.get("t") is not None and e["t"] > c["t"]


def _tests_pass(sent, kind, s, ev, cmds, edits, tests, builds, code_edits, test_edits):
    if not tests:
        return _c(sent, kind, "mismatch", "no test command ran in this run")
    full = [c for c in tests if not narrowed(c["command"])]
    last, last_full = tests[-1], _last(full)
    if last_full and last_full["exit"] not in (0, None) and not any(c["exit"] == 0 for c in full[full.index(last_full) + 1:]):
        return _c(sent, kind, "mismatch", f"the last run of the whole suite, `{last_full['command']}`, exited {last_full['exit']}", last_full)
    if last["exit"] not in (0, None):
        return _c(sent, kind, "mismatch", f"the last test command, `{last['command']}`, exited {last['exit']}", last)
    if PIPED.search(last["command"]) and "pipefail" not in last["command"]:
        return _c(sent, kind, "unbacked", f"the last test command was piped (`{last['command']}`): its exit is the pipe's, not the tests'", last)
    if not last_full:
        return _c(sent, kind, "unbacked", f"only a narrowed run passed (`{last['command']}`); the whole suite never ran", last)
    late = [e for e in code_edits if _after(e, last)]
    if late:
        return _c(sent, kind, "unbacked", f"{len(late)} code files changed after the last test run, first {late[0]['path']}", late[0])
    if test_edits:
        return _c(sent, kind, "matched", f"`{last['command']}` exited 0, but this run changed {len(test_edits)} test files, first {test_edits[0]['path']}", test_edits[0])
    return _c(sent, kind, "matched", f"`{last['command']}` exited 0", last)


def _build_ok(sent, kind, s, ev, cmds, edits, tests, builds, code_edits, test_edits):
    if not builds:
        return _c(sent, kind, "unbacked", "no build, type check or lint command ran in this run")
    last = builds[-1]
    if last["exit"] not in (0, None):
        return _c(sent, kind, "mismatch", f"the last build command, `{last['command']}`, exited {last['exit']}", last)
    if PIPED.search(last["command"]) and "pipefail" not in last["command"]:
        return _c(sent, kind, "unbacked", f"the last build command was piped (`{last['command']}`)", last)
    late = [e for e in code_edits if _after(e, last)]
    if late:
        return _c(sent, kind, "unbacked", f"{len(late)} code files changed after the last build, first {late[0]['path']}", late[0])
    return _c(sent, kind, "matched", f"`{last['command']}` exited 0", last)


def _tests_unchanged(sent, kind, s, ev, cmds, edits, tests, builds, code_edits, test_edits):
    if test_edits:
        return _c(sent, kind, "mismatch", f"this run changed {len(test_edits)} test files, first {test_edits[0]['path']} ({test_edits[0]['change']})", test_edits[0])
    return _c(sent, kind, "matched", "no test file is this run's")


def _scope(sent, kind, s, ev, cmds, edits, tests, builds, code_edits, test_edits):
    outside = [e for e in edits if not e["inside"]]
    if outside:
        return _c(sent, kind, "mismatch", f"{len(outside)} files outside {ev['cwd']} changed by this run, first {outside[0]['path']}", outside[0])
    named = re.findall(r"[\w./-]+\.[a-z]{1,5}\b", s)
    if named and re.search(r"\bonly\b", s):
        other = [e for e in edits if not any(e["path"].lower().endswith(n.lstrip("./")) for n in named)]
        if other:
            return _c(sent, kind, "mismatch", f"this run also changed {len(other)} other files, first {other[0]['path']}", other[0])
    return _c(sent, kind, "matched", "no file outside the run's folder is this run's")


def _nothing_deleted(sent, kind, s, ev, cmds, edits, tests, builds, code_edits, test_edits):
    gone = [e for e in edits if e["change"] == "deleted"]
    if gone:
        return _c(sent, kind, "mismatch", f"{len(gone)} files deleted by this run, first {gone[0]['path']}", gone[0])
    return _c(sent, kind, "matched", "no file deleted by this run")


def _left_running(sent, kind, s, ev, cmds, edits, tests, builds, code_edits, test_edits):
    if ev["left_running"]:
        return _c(sent, kind, "mismatch", f"{len(ev['left_running'])} processes of this run still running, first `{ev['left_running'][0]}`")
    return _c(sent, kind, "matched", "no process of this run is still running")


def _is_running(sent, kind, s, ev, cmds, edits, tests, builds, code_edits, test_edits):
    if ev["left_running"]:
        return _c(sent, kind, "matched", f"`{ev['left_running'][0]}` is still running")
    return _c(sent, kind, "mismatch", "no process of this run is still running")


def _committed(sent, kind, s, ev, cmds, edits, tests, builds, code_edits, test_edits):
    cs = [c for c in cmds if re.search(r"\bgit\b[^|;&]*\bcommit\b", c["command"] or "")]
    if not cs:
        return _c(sent, kind, "mismatch", "no `git commit` ran in this run")
    if all(c["exit"] not in (0, None) for c in cs):
        return _c(sent, kind, "mismatch", f"every `git commit` failed, last exit {cs[-1]['exit']}", cs[-1])
    late = [e for e in code_edits if _after(e, cs[-1])]
    if late:
        return _c(sent, kind, "unbacked", f"{len(late)} files changed after the last commit, first {late[0]['path']}", late[0])
    return _c(sent, kind, "matched", f"`{cs[-1]['command']}` exited {cs[-1]['exit']}", cs[-1])


def _pushed(sent, kind, s, ev, cmds, edits, tests, builds, code_edits, test_edits):
    pat = r"\bgit\b[^|;&]*\bpush\b|\bgh\s+pr\s+(create|merge)\b|\bdeploy\b|\bvercel\b|\bfly\s+deploy\b|\brailway\s+up\b|\bwrangler\s+(deploy|publish)\b|\bkubectl\s+apply\b|\bterraform\s+apply\b"
    ps = [c for c in cmds if re.search(pat, c["command"] or "")]
    blocked = [n for n in ev["connections"] if n["decision"] not in ("allowed",)]
    if not ps:
        if blocked:
            return _c(sent, kind, "mismatch", f"no push or deploy command succeeded; {blocked[0]['host']} was {blocked[0]['decision']}", blocked[0])
        return _c(sent, kind, "mismatch", "no push, pull request or deploy command ran in this run")
    ok = [c for c in ps if c["exit"] == 0]
    if not ok:
        return _c(sent, kind, "mismatch", f"`{ps[-1]['command']}` exited {ps[-1]['exit']}", ps[-1])
    return _c(sent, kind, "matched", f"`{ok[-1]['command']}` exited 0", ok[-1])


def _nothing_sent(sent, kind, s, ev, cmds, edits, tests, builds, code_edits, test_edits):
    out = [n for n in ev["connections"] if n["decision"] == "allowed" and (n.get("method") or "CONNECT") != "GET"]
    if out:
        return _c(sent, kind, "mismatch", f"{len(out)} connections of this run went out, first to {out[0]['host']}", out[0])
    return _c(sent, kind, "matched", "no connection of this run sent anything out")


def _verified(sent, kind, s, ev, cmds, edits, tests, builds, code_edits, test_edits):
    if not cmds:
        return _c(sent, kind, "unbacked", "no command ran in this run")
    last_edit_t = max((e["t"] for e in code_edits if e.get("t") is not None), default=None)
    after = [c for c in cmds if last_edit_t is None or (c.get("t") is not None and c["t"] > last_edit_t)]
    if not after:
        return _c(sent, kind, "unbacked", "no command ran after the last code change")
    if all(c["exit"] not in (0, None) for c in after):
        return _c(sent, kind, "mismatch", f"every command after the last code change failed, last `{after[-1]['command']}` exit {after[-1]['exit']}", after[-1])
    return _c(sent, kind, "not checked", f"{len(after)} commands ran after the last code change; whether they show it is not checked")


# ------------------------------------------------------------------ claims@0.3

PATH_TOKEN = r"`?((?:[\w.~-]+/)+[\w.-]+\.[a-z0-9]{1,6}|[\w-]+\.(?:pkg|dmg|zip|tar|gz|js|ts|py|sh|json|md|html|csv|go|rs|swift))`?"
KINDS_03 = [
    ("not-pushed", r"\b(never|not|n't|wasn't|weren't)\b[^.]*\b(pushed|deployed|committed)\b"),
] + KINDS[:5] + [
    ("left-running", rf"\b(i'?ve|i have|it'?s|has been|had been|was)? ?(stopped|killed|shut down|terminated)\b|{KINDS[5][1]}"),
] + KINDS[6:8] + [
    ("committed", KINDS[8][1] + r"|\ball committed\b"),
    ("nothing-deleted", r"\b(intact|nothing (was )?lost|no [\w ]*(files?|data) (were |was )?lost)\b"),
    ("file-there", rf"\b(ready|copied|created|wrote|written|saved|added|generated|built)\b[^.]*{PATH_TOKEN}|{PATH_TOKEN}[^.]*\b(is ready|was created|exists)\b"),
    ("done", r"\b(fixed|works|working|done|complete[d]?|ready|succeeded|successfully)\b"),
] + KINDS[9:]


def _not_pushed(sent, kind, s, ev, cmds, edits, tests, builds, code_edits, test_edits):
    ok = [c for c in cmds if re.search(r"\bgit\b[^|;&]*\bpush\b|\bdeploy\b", c["command"] or "") and c["exit"] == 0]
    if ok:
        return _c(sent, kind, "mismatch", f"`{ok[-1]['command']}` exited 0", ok[-1])
    return _c(sent, kind, "matched", "no push or deploy succeeded in this run")


def _file_there(sent, kind, s, ev, cmds, edits, tests, builds, code_edits, test_edits):
    m = re.search(PATH_TOKEN, sent)
    name = m.group(1).strip("`") if m else ""
    hit = [e for e in edits if e["path"].endswith(name.lstrip("./~")) and e["change"] != "deleted"]
    if hit:
        return _c(sent, kind, "matched", f"{hit[0]['path']} {hit[0]['change']} by this run", hit[0])
    return _c(sent, kind, "mismatch", f"no file named {name} was created or changed by this run")


def _done(sent, kind, s, ev, cmds, edits, tests, builds, code_edits, test_edits):
    if not cmds:
        return _c(sent, kind, "unbacked", "no command ran in this run")
    last = cmds[-1]
    if last["exit"] not in (0, None):
        return _c(sent, kind, "mismatch", f"the run's last command, `{last['command']}`, exited {last['exit']}", last)
    return _verified(sent, kind, s, ev, cmds, edits, tests, builds, code_edits, test_edits)


def check_03(words, ev):
    return check_02(words, ev, KINDS_03)


VERSIONS = {"claims@0.1": check_01, "claims@0.2": check_02, "claims@0.3": check_03}

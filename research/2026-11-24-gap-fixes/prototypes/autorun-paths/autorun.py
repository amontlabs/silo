#!/usr/bin/env python3
"""List the changes between two commits to files that a tool on the Mac runs without being asked.

    python3 autorun.py <repo> <old> <new> [--json] [--data autorun-paths.json]

<old> may be 0000000000000000000000000000000000000000 (a new branch): the change is then taken from
the empty tree. The repository is only read with plumbing commands, and Git is told never to run a
diff driver or textconv the repository could name. Exit code: 0 nothing a rule would hold,
1 a change a rule would hold (marked HOLD), 2 the input could not be read.

Each finding is a fact about the two trees: the path, how it changed, and for files with a content
check, which keys changed. Which tool reads the path comes from the data file, as ecosystem data.
"""
import fnmatch, json, os, re, subprocess, sys, tomllib, unicodedata

ZERO = "0" * 40
EMPTY_TREE = "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
HERE = os.path.dirname(os.path.abspath(__file__))


def git(repo, *args, data=False):
    env = dict(os.environ, GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL="/dev/null", GIT_TERMINAL_PROMPT="0")
    p = subprocess.run(["git", "-C", repo, "-c", "core.fsmonitor=false", "-c", "diff.external=", *args],
                       capture_output=True, env=env)
    if p.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)}: {p.stderr.decode(errors='replace').strip()}")
    return p.stdout if data else p.stdout.decode("utf-8", "surrogateescape")


def fold(path):
    """The Mac's disk (APFS, default settings) is case-insensitive and normalisation-insensitive."""
    return unicodedata.normalize("NFC", path).casefold()


def glob_re(pattern):
    out, i = "", 0
    p = fold(pattern)
    while i < len(p):
        if p.startswith("**/", i):
            out += "(?:.*/)?"; i += 3
        elif p.startswith("/**", i) and i + 3 == len(p):
            out += "/.*"; i += 3
        elif p[i] == "*":
            out += "[^/]*"; i += 1
        elif p[i] == "?":
            out += "[^/]"; i += 1
        else:
            out += re.escape(p[i]); i += 1
    return re.compile(out + r"\Z")


def load_data(path):
    d = json.load(open(path))
    for e in d["entries"]:
        e["_re"] = [glob_re(p) for p in e["paths"]]
    return d


def match(data, path):
    cache = data.setdefault("_cache", {})
    if path not in cache:
        f = fold(path)
        cache[path] = [e for e in data["entries"] if any(r.match(f) for r in e["_re"])]
    return cache[path]


# --- reading files -------------------------------------------------------------------------------

def jsonc(text):
    """JSON with comments and trailing commas, as VS Code reads it. Returns None if unreadable."""
    if text is None:
        return None
    out, i, n, s = [], 0, len(text), None
    while i < n:
        c = text[i]
        if s:
            out.append(c)
            if c == "\\" and i + 1 < n:
                out.append(text[i + 1]); i += 2; continue
            if c == s:
                s = None
            i += 1
        elif c == '"':
            s = c; out.append(c); i += 1
        elif text.startswith("//", i):
            j = text.find("\n", i); i = n if j < 0 else j
        elif text.startswith("/*", i):
            j = text.find("*/", i + 2); i = n if j < 0 else j + 2
        else:
            out.append(c); i += 1
    cleaned = re.sub(r",(\s*[}\]])", r"\1", "".join(out))
    try:
        return json.loads(cleaned)
    except ValueError:
        return None


def get(obj, dotted):
    for k in dotted.split("."):
        if not isinstance(obj, dict) or k not in obj:
            return None
        obj = obj[k]
    return obj


def walk_commands(obj, pattern):
    """Values at a dotted pattern where * matches any key or list index."""
    parts = pattern.split(".")
    def rec(o, ps):
        if not ps:
            if isinstance(o, str):
                yield o
            elif isinstance(o, list):
                for x in o:
                    if isinstance(x, str):
                        yield x
            return
        k, rest = ps[0], ps[1:]
        if k == "*":
            items = o.values() if isinstance(o, dict) else o if isinstance(o, list) else []
            for v in items:
                yield from rec(v, rest)
        elif isinstance(o, dict) and k in o:
            yield from rec(o[k], rest)
    yield from rec(obj, parts)


def dumps(v):
    return json.dumps(v, sort_keys=True)


def key_changes(old, new, keys):
    out = []
    for k in keys:
        a, b = get(old, k) if old is not None else None, get(new, k) if new is not None else None
        if dumps(a) != dumps(b):
            what = "added" if a is None else "removed" if b is None else "changed"
            out.append(f"{k} {what}")
    return out


def flatten(obj, prefix=""):
    out = {}
    if isinstance(obj, dict):
        for k, v in obj.items():
            key = f"{prefix}{k}"
            if isinstance(v, dict) and not k.startswith("["):
                out.update(flatten(v, key + "."))
            elif isinstance(v, dict):
                out.update(flatten(v, key + "."))
            else:
                out[key] = v
    return out


# --- content checks: each returns (details, commands in the new version) -------------------------

def check_any(entry, old, new):
    return ([], [])


def check_json_keys(entry, old, new):
    o, n = jsonc(old), jsonc(new)
    if (old is not None and o is None) or (new is not None and n is None):
        return (["file could not be read as JSON: every change counts"], [])
    det = key_changes(o, n, entry["check"]["keys"])
    cmds = [c for pat in entry.get("commands_at", []) for c in walk_commands(n or {}, pat)]
    old_cmds = {c for pat in entry.get("commands_at", []) for c in walk_commands(o or {}, pat)}
    det += [f"new command: {c}" for c in cmds if c not in old_cmds]
    return (det, cmds) if det else (None, cmds)


def tasks_closure(doc):
    tasks = (doc or {}).get("tasks") or []
    by_label = {t.get("label"): t for t in tasks if isinstance(t, dict)}
    auto = [t for t in tasks if isinstance(t, dict) and get(t, "runOptions.runOn") == "folderOpen"]
    seen, stack = {}, list(auto)
    while stack:
        t = stack.pop()
        lab = t.get("label") or dumps(t)
        if lab in seen:
            continue
        seen[lab] = t
        deps = t.get("dependsOn") or []
        for d in [deps] if isinstance(deps, str) else deps:
            if isinstance(d, str) and d in by_label:
                stack.append(by_label[d])
    return seen


def task_cmd(t):
    c = t.get("command", "")
    args = t.get("args") or []
    return " ".join([c if isinstance(c, str) else dumps(c)] + [a if isinstance(a, str) else dumps(a) for a in args])


def check_vscode_tasks(entry, old, new, doc_old=None, doc_new=None):
    o = doc_old if doc_old is not None else jsonc(old)
    n = doc_new if doc_new is not None else jsonc(new)
    if (old is not None and o is None) or (new is not None and n is None):
        return (["file could not be read as JSON: every change counts"], [])
    a, b = tasks_closure(o), tasks_closure(n)
    det = []
    for lab, t in b.items():
        if lab not in a:
            det.append(f"task '{lab}' runs on folder open (added): {task_cmd(t)}")
        elif dumps(a[lab]) != dumps(t):
            det.append(f"task '{lab}' runs on folder open (changed): {task_cmd(t)}")
    for lab in a:
        if lab not in b:
            det.append(f"task '{lab}' no longer runs on folder open")
    cmds = [task_cmd(t) for t in b.values()]
    return (det, cmds) if det else (None, cmds)


def check_vscode_settings(entry, old, new, data=None, doc_old=None, doc_new=None):
    o = doc_old if doc_old is not None else jsonc(old)
    n = doc_new if doc_new is not None else jsonc(new)
    if (old is not None and o is None) or (new is not None and n is None):
        return (["file could not be read as JSON: every change counts"], [])
    fo, fn = flatten(o or {}), flatten(n or {})
    runs = (data or {}).get("vscode_settings_that_run", [])
    det = []
    for k in sorted(set(fo) | set(fn)):
        if dumps(fo.get(k)) != dumps(fn.get(k)):
            bare = re.sub(r"^\[[^\]]*\]\.", "", k)
            known = any(bare == r or (r.endswith(".") and bare.startswith(r)) for r in runs)
            det.append(f"{k} {'changed' if k in fo and k in fn else 'added' if k in fn else 'removed'}"
                       + (" (names a program the editor runs)" if known else ""))
    cmds = [str(v) for k, v in fn.items() if any(k == r or (r.endswith(".") and k.startswith(r)) for r in runs)]
    if not det:
        return (None, cmds)
    if not any("(names a program" in d for d in det):
        return (["only settings that name no known program: " + ", ".join(d.split(" ")[0] for d in det)], cmds)
    return (det, cmds)


def check_vscode_workspace(entry, old, new, data=None):
    o, n = jsonc(old), jsonc(new)
    if (old is not None and o is None) or (new is not None and n is None):
        return (["file could not be read as JSON: every change counts"], [])
    d1, c1 = check_vscode_tasks(entry, old, new, (o or {}).get("tasks") or {}, (n or {}).get("tasks") or {})
    d2, c2 = check_vscode_settings(entry, old, new, data, (o or {}).get("settings") or {}, (n or {}).get("settings") or {})
    det = (d1 or []) + (d2 or [])
    return (det or None, c1 + c2)


LIFECYCLE = ["preinstall", "install", "postinstall", "preprepare", "prepare", "postprepare", "prepublish",
             "prepack", "postpack", "dependencies"]
HOOK_KEYS = ["packageManager", "simple-git-hooks", "husky", "gitHooks", "pre-commit", "ghooks", "lint-staged"]
NONREG = re.compile(r"^(git\+|git:|github:|gitlab:|bitbucket:|https?:|file:|link:|portal:)|^[\w.-]+/[\w.-]+(#.*)?$")


def check_package_json(entry, old, new):
    o, n = jsonc(old), jsonc(new)
    if (old is not None and o is None) or (new is not None and n is None):
        return (["file could not be read as JSON: every change counts"], [])
    o, n = o or {}, n or {}
    det = key_changes(o, n, [f"scripts.{s}" for s in LIFECYCLE] + [k for k in HOOK_KEYS if k != "packageManager"])
    pm_a, pm_b = o.get("packageManager"), n.get("packageManager")
    pm = None
    if pm_a != pm_b and isinstance(pm_b, str):
        pm = f"packageManager {'changed' if pm_a else 'added'}: {pm_b[:60]}"
        if "://" in pm_b:
            det.append(pm + " (a URL: corepack runs what it serves)")
            pm = None
    for sec in ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies", "overrides", "resolutions"]:
        a, b = flatten(o.get(sec) or {}), flatten(n.get(sec) or {})
        for name, spec in b.items():
            if isinstance(spec, str) and NONREG.search(spec) and a.get(name) != spec \
                    and not re.match(r"^(file|link|portal):\.{0,2}/", spec):  # a folder of the same repository
                det.append(f"{sec}.{name} fetched from outside the registry: {spec}")
    cmds = [v for s in LIFECYCLE if isinstance(v := get(n, f"scripts.{s}"), str)]
    if not det and pm:
        return (["listed only: " + pm + " (corepack fetches that version from the npm registry)"], cmds)
    return (det + ([pm] if pm else []), cmds) if det else (None, cmds)


def check_ini_keys(entry, old, new):
    def parse(t):
        d = {}
        for line in (t or "").splitlines():
            line = line.strip()
            if line and not line.startswith(("#", ";")) and "=" in line:
                k, v = line.split("=", 1)
                d[k.strip()] = v.strip()
        return d
    a, b = parse(old), parse(new)
    det = [f"{k} {'added' if k not in a else 'removed' if k not in b else 'changed'}"
           for k in sorted(set(a) | set(b)) if a.get(k) != b.get(k)]
    return (det or None, [])


def check_pyproject(entry, old, new, path=""):
    if not path.lower().endswith("pyproject.toml"):
        return ([], [])
    try:
        o = tomllib.loads(old) if old is not None else {}
        n = tomllib.loads(new) if new is not None else {}
    except tomllib.TOMLDecodeError:
        return (["file could not be read as TOML: every change counts"], [])
    det = key_changes(o, n, ["build-system", "tool.uv.sources", "tool.setuptools.cmdclass", "tool.hatch.build.hooks", "tool.poetry.build"])
    return (det or None, [])


def frontmatter_keys(text, keys):
    """The lines of the given top-level keys in a Markdown file's YAML frontmatter."""
    if not text or not text.startswith("---"):
        return {}
    end = text.find("\n---", 3)
    block = text[3:end if end > 0 else len(text)].splitlines()
    out, cur = {}, None
    for line in block:
        m = re.match(r"^([A-Za-z_-]+)\s*:", line)
        if m:
            cur = m.group(1) if m.group(1) in keys else None
        if cur:
            out.setdefault(cur, []).append(line)
    return out


def check_frontmatter(entry, old, new):
    """Skills and subagents: only frontmatter that runs something decides; the text is 'instructs'."""
    keys = entry["check"]["keys"]
    a, b = frontmatter_keys(old, keys), frontmatter_keys(new, keys)
    det = [f"frontmatter {k} {'added' if k not in a else 'removed' if k not in b else 'changed'}"
           for k in keys if a.get(k) != b.get(k)]
    if det:
        return (det, [])
    return (["text only: goes into the prompt of the agent on the Mac"], [])


def check_yaml_keys(entry, old, new):
    """Top-level keys of a YAML file, compared as text: no YAML parser is needed for that."""
    keys = entry["check"]["keys"]
    def blocks(t):
        if t is None:
            return {}
        return frontmatter_keys("---\n" + t + "\n---", keys)
    a, b = blocks(old), blocks(new)
    det = [f"{k} {'added' if k not in a else 'removed' if k not in b else 'changed'}" for k in keys if a.get(k) != b.get(k)]
    return (det or None, [])


def check_version_file(entry, old, new, path=""):
    """A version number picks an official build; anything else (a path, a custom toolchain) can run anything."""
    if new is None:
        return (None, [])
    if path.lower().endswith(".toml"):
        try:
            doc = tomllib.loads(new)
        except tomllib.TOMLDecodeError:
            return (["file could not be read as TOML: every change counts"], [])
        if get(doc, "toolchain.path") is not None:
            return (["toolchain.path names a toolchain folder"], [])
        return (["version only: " + str(get(doc, "toolchain.channel"))], [])
    v = new.strip()
    if re.fullmatch(r"[\w.+\-/ *~^<>=]{1,40}", v) and "/" not in v.strip("v"):
        return ([f"version only: {v}"], [])
    return ([f"not a plain version: {v[:60]}"], [])


def check_list(entry, old, new):
    return (["listed only: runs nothing by itself"], [])


CHECKS = {"list": check_list, "beside": check_any, "yaml-keys": check_yaml_keys, "version-file": check_version_file, "frontmatter": check_frontmatter, "any": check_any, "json-keys": check_json_keys, "vscode-tasks": check_vscode_tasks,
          "vscode-settings": check_vscode_settings, "vscode-workspace": check_vscode_workspace,
          "package-json": check_package_json, "ini-keys": check_ini_keys, "pyproject": check_pyproject}


# --- the two trees -------------------------------------------------------------------------------

def blob(repo, rev, path):
    try:
        return git(repo, "cat-file", "blob", f"{rev}:{path}", data=True).decode("utf-8", "replace")
    except RuntimeError:
        return None


_blobs = {}


def blob_by_id(repo, sha):
    """Blobs are named by their content, so one read serves every commit that holds the same file."""
    if sha not in _blobs:
        try:
            _blobs[sha] = git(repo, "cat-file", "blob", sha, data=True).decode("utf-8", "replace")
        except RuntimeError:
            _blobs[sha] = None
    return _blobs[sha]


def ls_tree(repo, rev):
    """path -> (mode, sha) for every entry of the tree."""
    out = {}
    if rev == EMPTY_TREE:
        return out
    for rec in git(repo, "ls-tree", "-r", "-z", "--full-tree", rev).split("\0"):
        if rec:
            meta, path = rec.split("\t", 1)
            mode, _, sha = meta.split(" ")
            out[path] = (mode, sha)
    return out


def changed(repo, old, new):
    raw = git(repo, "diff", "--name-status", "-z", "--no-renames", "--no-ext-diff", "--no-textconv", old, new)
    parts = [p for p in raw.split("\0") if p]
    return [(parts[i], parts[i + 1]) for i in range(0, len(parts), 2)]


PATHLIKE = re.compile(r"(?:\.{0,2}/)?[\w.@-]+(?:/[\w.@-]+)*\.(?:m?js|cjs|ts|mts|sh|bash|zsh|py|rb|pl|ps1|cmd|bat|exe|bin|jar)\b|(?:\.{0,2}/)?(?:[\w.@-]+/)+[\w.@-]+")


def referenced_paths(cmds, tree):
    """Files of the repository named literally in a command that runs on its own."""
    folded = {fold(p): p for p in tree}
    refs = set()
    for c in cmds:
        for tok in PATHLIKE.findall(c):
            t = fold(os.path.normpath(tok.lstrip("./") if tok.startswith("./") else tok))
            if t in folded:
                refs.add(folded[t])
    return refs


SUFFIXES = ["", "/settings.json", "/tasks.json", "/mcp.json", "/config.toml", "/hooks.json", "/pre-commit", "/skills/x/SKILL.md"]


def at_location(data, p):
    """Entries a link at p would stand in for: the file itself or a folder that holds one."""
    return [e for suf in SUFFIXES for e in match(data, p + suf)]


def analyse(repo, old, new, data):
    if old == ZERO:
        old = EMPTY_TREE
    tree_new, tree_old = ls_tree(repo, new), ls_tree(repo, old)
    changes = changed(repo, old, new)
    findings, standing_cmds = [], []

    # Symlinks: a link committed at an auto-run location makes its target's files count as that location.
    aliases = {}
    for p, (mode, sha) in tree_new.items():
        if mode == "120000":
            target = blob(repo, new, p) or ""
            dest = os.path.normpath(os.path.join(os.path.dirname(p), target))
            if at_location(data, p):
                aliases[dest] = p
    def aliased(path):
        for dest, link in aliases.items():
            if path == dest or path.startswith(dest + "/"):
                return link + path[len(dest):]
        return None

    for status, path in changes:
        names = [path] + ([a] if (a := aliased(path)) else [])
        for name in names:
            for e in match(data, name):
                if e["check"]["kind"] == "beside":
                    # Like ecosystem-data: a build.rs is a build script only beside its Cargo.toml.
                    sib = os.path.join(os.path.dirname(name), e["check"]["file"])
                    if sib not in tree_new and sib not in tree_old:
                        continue
                old_t = blob(repo, old, path) if status != "A" else None
                new_t = blob(repo, new, path) if status != "D" else None
                fn = CHECKS[e["check"]["kind"]]
                if fn is check_vscode_settings or fn is check_vscode_workspace:
                    det, cmds = fn(e, old_t, new_t, data)
                elif fn is check_pyproject or fn is check_version_file:
                    det, cmds = fn(e, old_t, new_t, path)
                else:
                    det, cmds = fn(e, old_t, new_t)
                if det is None:
                    continue  # content check: nothing that runs changed
                mode = tree_new.get(path, tree_old.get(path, ("", "")))[0]
                findings.append({"path": path, "as": name if name != path else None, "status": status,
                                 "symlink": mode == "120000", "entry": e["id"], "tool": e["tool"], "class": e["class"],
                                 "trigger": e["trigger"], "gate": e["gate"], "details": det,
                                 "hold": e["class"] in ("runs", "redirects") and status != "D"
                                 and not any(d.startswith(("only settings that name no known program", "text only", "version only", "listed only")) for d in det)
                                 and not (det and all(d.endswith((" removed", "no longer runs on folder open")) for d in det))})

    # A link added or changed at an auto-run location moves that location.
    for status, path in changes:
        mode = tree_new.get(path, ("", ""))[0]
        if mode == "120000" and status != "D" and (es := at_location(data, path)):
            e = es[0]
            findings.append({"path": path, "as": None, "status": status, "symlink": True, "entry": e["id"], "tool": e["tool"],
                             "class": e["class"], "trigger": e["trigger"], "gate": e["gate"],
                             "details": [f"link to {blob(repo, new, path)}: the files there count as {path}"], "hold": True})

    # Commands that run on their own in the new tree, changed or not: a worm's second visit changes
    # only the file such a command names (node .claude/setup.mjs), and that is the change to list.
    standing_cmds = []
    for p, (mode, sha) in tree_new.items():
        for e in match(data, p):
            if e["class"] != "runs" or mode == "120000" or e["check"]["kind"] == "beside":
                continue
            t = blob_by_id(repo, sha)
            if t is None or len(t) > 65536:
                continue
            kind = e["check"]["kind"]
            if kind == "any":
                cmds = [t]  # a hook script, an .envrc: the file is the command
            elif kind in ("vscode-settings", "vscode-workspace"):
                cmds = CHECKS[kind](e, t, t, data)[1]
            elif kind in ("pyproject", "yaml-keys", "version-file"):
                cmds = []
            else:
                cmds = CHECKS[kind](e, t, t)[1]
            standing_cmds += [(e, p, c) for c in cmds]

    # A file named in a command that runs on its own changed.
    changed_paths = {p for _, p in changes}
    listed = {f["path"] for f in findings}
    for e, src, c in standing_cmds:
        for ref in referenced_paths([c], tree_new):
            if ref in changed_paths and ref not in listed:
                listed.add(ref)
                findings.append({"path": ref, "as": None, "status": dict((p, s) for s, p in changes)[ref], "symlink": False,
                                 "entry": e["id"], "tool": e["tool"], "class": "runs", "trigger": f"run by {src}: {c}",
                                 "gate": e["gate"], "details": [f"named in a command of {src}"],
                                 "hold": dict((p, s) for s, p in changes)[ref] != "D"})

    # Two paths that are one file on the Mac's case-insensitive disk.
    seen = {}
    for p in tree_new:
        seen.setdefault(fold(p), []).append(p)
    for group in seen.values():
        if len(group) > 1 and any(p in changed_paths for p in group):
            findings.append({"path": group[0], "as": None, "status": "A", "symlink": False, "entry": "case-collision",
                             "tool": "the Mac's disk", "class": "runs" if any(match(data, p) for p in group) else "redirects",
                             "trigger": "checkout on a case-insensitive disk keeps only one of them", "gate": "none",
                             "details": ["same file on the Mac: " + ", ".join(group)], "hold": True})
    return findings


def render(findings):
    order = {"runs": 0, "redirects": 1, "instructs": 2}
    lines = []
    for f in sorted(findings, key=lambda f: (order.get(f["class"], 9), f["path"])):
        st = {"A": "added", "M": "changed", "D": "removed", "T": "type changed"}.get(f["status"][0], f["status"])
        lines.append(("HOLD " if f["hold"] else "list ") + f"[{f['class']}] {f['path']} {st}" + (f" (read on the Mac as {f['as']})" if f["as"] else "")
                     + (" (symlink)" if f["symlink"] else ""))
        lines.append(f"    tool: {f['tool']}; when: {f['trigger']}")
        lines.append(f"    asks first: {f['gate']}")
        for d in f["details"]:
            lines.append(f"    - {d}")
    return "\n".join(lines) if lines else "nothing that a tool runs on its own changed"


def main(argv):
    args = [a for a in argv if not a.startswith("--")]
    dpath = os.path.join(HERE, "autorun-paths.json")
    if "--data" in argv:
        dpath = argv[argv.index("--data") + 1]; args.remove(dpath)
    if len(args) != 3:
        print(__doc__); return 2
    repo, old, new = args
    try:
        findings = analyse(repo, old, new, load_data(dpath))
    except RuntimeError as e:
        print(f"could not read the change: {e}", file=sys.stderr)
        return 2
    print(json.dumps(findings, indent=1) if "--json" in argv else render(findings))
    return 1 if any(f["hold"] for f in findings) else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

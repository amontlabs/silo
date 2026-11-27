"""`silo put-back`, by time and path, with a preview and a confirm, for any change.

    python3 putback.py <night> [computer] <time> <path>... [--yes] [--json]
    python3 putback.py <night> [computer] <time> <path>... --plan <file>    # preview only, saved
    python3 putback.py <night> --confirm <file>                            # carry out that preview
    options: --swap exchange|rename   (exchange by default; rename = #370's move aside, then rename)

<time> is a time of day, a checkpoint (ck12) or a change (main:f479, the checkpoint before it).
<path> may start with ~. It follows #370 step 5's six steps:

  1. Read    each path in the checkpoint; a path not there is refused before anything happens
  2. Show    what comes back, what it replaces now and who changed that since, by #399's rules
             under option A (no processes), and what changed after the last checkpoint
  3. Save    a checkpoint of now, reason `before put back`, kept until you remove it
  4. Copy    the paths' contents beside each target, in <parent>/.silo-put-back-<id>/
  5. Swap    the copy into place, after checking that nothing changed since the preview
  6. Record  a control fact: you put back these paths from this checkpoint

Two things are added to #370's spec, each measured by test_putback.py:
  - the confirm is bound to what the preview showed: a fingerprint of the paths as they were shown.
    A write between the preview and the swap makes the put-back stop before it swaps, and say so,
    instead of losing that write in silence (#370: "a write between step 2 and step 5 is lost").
  - the swap exchanges the two names in one call (Linux renameat2 RENAME_EXCHANGE; macOS
    renamex_np RENAME_SWAP), so a program never finds the path missing.

The computer is a folder here, as in #371; on a Mac steps 2, 4 and 5 run inside main over SSH.
"""
import ctypes, ctypes.util, hashlib, json, os, shutil, stat, sys, time

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import night_a as N  # noqa: E402

R = N.R
EXIT_REFUSED, EXIT_CHANGED, EXIT_NO = 2, 3, 4

_libc = ctypes.CDLL(ctypes.util.find_library("c"), use_errno=True)
AT_FDCWD, RENAME_EXCHANGE = -100, 2


def exchange(a, b):
    """Swap two names in one call: no moment where either is missing."""
    if _libc.renameat2(AT_FDCWD, os.fsencode(a), AT_FDCWD, os.fsencode(b), RENAME_EXCHANGE) != 0:
        e = ctypes.get_errno()
        raise OSError(e, os.strerror(e), a)


# ---------------------------------------------------------------- listing
def entry(p):
    """One path as the checkpoint manifests hold it, read from the computer now."""
    try:
        st = os.lstat(p)
    except FileNotFoundError:
        return None
    if stat.S_ISLNK(st.st_mode):
        return {"t": "l", "to": os.readlink(p)}
    if stat.S_ISDIR(st.st_mode):
        return {"t": "d"}
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return {"t": "f", "size": st.st_size, "sha": h.hexdigest(), "mode": st.st_mode & 0o777}


def live(path, staging=False):
    """Every file under a path now: {path: entry}. Folders are not entries, as in #371's manifests.
    Put-back's staging folders are left out unless `staging`."""
    e = entry(path)
    if e is None:
        return {}
    if e["t"] != "d":
        return {path: e}
    out = {}
    for dp, dns, fns in os.walk(path):
        if not staging:
            dns[:] = [d for d in dns if not d.startswith(".silo-put-back-")]
        for fn in fns + [d for d in dns if os.path.islink(os.path.join(dp, d))]:
            p = os.path.join(dp, fn)
            x = entry(p)
            if x:
                out[p] = x
    return out


def under(m, path):
    return {p: e for p, e in m.items() if p == path or p.startswith(path + "/")}


def fingerprint(entries):
    return hashlib.sha256(json.dumps(sorted(entries.items()), sort_keys=True).encode()).hexdigest()


def same(a, b):
    if a is None or b is None:
        return a is b
    return (a.get("t"), a.get("sha"), a.get("mode"), a.get("to")) == (b.get("t"), b.get("sha"), b.get("mode"), b.get("to"))


# ---------------------------------------------------------------- 1 and 2: read and show
def preview(night, computer, when, paths):
    t, cid, n = night.resolve_time(computer, when)
    then_all = night.manifest(computer, n)
    cks = night.checkpoints(computer)
    newest = night.manifest(computer, cks[-1][2])
    plan = {"v": 1, "night": night.folder, "computer": computer, "from": cid, "from_time": t, "paths": [], "made": time.time()}
    for raw in paths:
        p = night.untilde(computer, raw)
        then = under(then_all, p)
        if not then:
            raise ValueError(f"{raw} is not in {computer} at {N.hm(t)} ({cid}): nothing to put back")
        now = live(p)
        rows = []
        for q in sorted(set(then) | set(now)):
            a, b = then.get(q), now.get(q)
            if same(a, b):
                continue
            what = "back" if b is None else "remove" if a is None else "replace"
            row = {"path": q, "what": what, "then": a, "now": b}
            if not same(newest.get(q), b):
                row["after_last_checkpoint"] = True     # not in any checkpoint yet; step 3 saves it
            if what != "back" or True:
                row["since"] = changes_since(night, computer, q, n)
            if a and a["t"] == "f" and not night.content(a["sha"]):
                row["unreadable"] = True
            rows.append(row)
        plan["paths"].append({"path": p, "shown": raw, "is_dir": len(then) > 1 or p not in then,
                              "files_then": len(then), "bytes_then": sum(e.get("size", 0) for e in then.values()),
                              "rows": rows, "unchanged": len(set(then) & set(now)) - sum(1 for r in rows if r["what"] == "replace"),
                              "fingerprint": fingerprint(now)})
    return plan


def changes_since(night, computer, path, n):
    """The file facts of a path after checkpoint n, each with its author as option A reads it."""
    out = []
    for f in night.facts(computer, "file"):
        if f["path"] != path or int(f["between"][0].split("@ck")[1]) < n:
            continue
        a = night.author(f["id"])
        out.append({"fact": f["id"], "change": f["change"], "between": f["between"], "how": a["how"],
                    "by": a["by"] if a["strong"] else None, "tool_call": a["tool_call"] if a["strong"] else None,
                    "while": sorted({c.conversation for c in a["candidates"]})})
    return out


def who(night, row):
    """One line: who made the change a put-back would undo."""
    s = row.get("since") or []
    if not s:
        return "not in the record yet: changed after the last checkpoint" if row.get("after_last_checkpoint") else "no change in the record"
    last = s[-1]
    if last["by"]:
        tc = night.call.get(last["tool_call"]) if last["tool_call"] else None
        return f"◇ {night.label(last['by'])}" + (f", {N.mark(N.words(night.summary(tc), 50))}" if tc else "")
    if last["while"]:
        return "◇ not known from the traffic; while " + ", ".join(night.label(c) for c in last["while"]) + " ran"
    return "not known: no tool call was open"


def render(night, plan):
    c = plan["computer"]
    out = []
    lost = []
    for P in plan["paths"]:
        rows = P["rows"]
        back = [r for r in rows if r["what"] == "back"]
        repl = [r for r in rows if r["what"] == "replace"]
        rem = [r for r in rows if r["what"] == "remove"]
        size = sum((r["then"] or {}).get("size", 0) for r in back + repl)
        shown = N.safe(P["shown"])
        out.append(f"{shown} as it was at {N.hm(plan['from_time'])} ({plan['from']}): {P['files_then']} file{'s' * (P['files_then'] != 1)}, {human(P['bytes_then'])}")
        if not rows:
            out.append("  It is the same now. Nothing to put back.")
            continue
        if back:
            out.append(f"  comes back   {len(back):>5} file{'s' * (len(back) != 1)} deleted since")
        if repl or rem:
            out.append(f"  undoes       {len(repl) + len(rem):>5} change{'s' * (len(repl) + len(rem) != 1)} made since, which leave {c}")
        groups = {}
        for r in rows:
            groups.setdefault(who(night, r), []).append(r)
        out.append("  by")
        for w, rs in groups.items():
            out.append(f"    {w}")
            for r in rs[:4]:
                verb = {"back": "deleted", "replace": "changed", "remove": "created"}[r["what"]]
                out.append(f"      {verb:<8} {night.tilde(c, r['path'])}" + ("   (after the last checkpoint)" if r.get("after_last_checkpoint") else ""))
            if len(rs) > 4:
                kinds = {}
                for r in rs[4:]:
                    kinds[r["what"]] = kinds.get(r["what"], 0) + 1
                out.append("      … " + ", ".join(f"{k} more {dict(back='deleted', replace='changed', remove='created')[w2]}" for w2, k in kinds.items()))
        lost += repl + rem
        if P["unchanged"]:
            out.append(f"  unchanged    {P['unchanged']:>5}")
        bad = [r for r in rows if r.get("unreadable")]
        if bad:
            out.append(f"  not readable {len(bad):>5}  named here and not copied: " + ", ".join(night.tilde(c, r["path"]) for r in bad[:3]))
        out.append(f"  {human(size)} to copy")
    if not any(P["rows"] for P in plan["paths"]):
        return "\n".join(out), False
    out.append("")
    if lost:
        out.append(f"What these changes made goes to the checkpoint Silo saves first: `silo look <that checkpoint> <path>` shows it.")
    else:
        out.append("Nothing in main is replaced.")
    out.append(f"Main keeps running. Silo saves a checkpoint of now first, kept until you remove it.")
    out.append("If any of these paths changes before the copy is in place, Silo stops and shows you again.")
    return "\n".join(out), True


def human(n):
    for u in ("B", "KB", "MB", "GB"):
        if n < 1024 or u == "GB":
            return f"{n:.0f} {u}" if u == "B" else f"{n:.1f} {u}"
        n /= 1024


# ---------------------------------------------------------------- 3 to 6
def disk_of(night, computer):
    return os.path.join(night.folder, "computers", computer)


def save(night, computer, why, by="you", staging=False):
    """A checkpoint of the whole computer now, in #371's format, with its comparison to the one before."""
    w = R.Writer(night.root, computer)
    cks = night.checkpoints(computer)
    m = {}
    cas = os.path.join(night.root, "cas")
    for p, e in live(disk_of(night, computer), staging).items():
        if e["t"] == "f":
            dst = os.path.join(cas, e["sha"])
            if not os.path.exists(dst):
                shutil.copyfile(p, dst + ".tmp"); os.replace(dst + ".tmp", dst)
        m[p] = e
    n = cks[-1][2] + 1
    cid = f"{computer}@ck{n}"
    tmp = os.path.join(night.root, computer, "checkpoints", f".ck{n}.json.tmp")
    json.dump(m, open(tmp, "w")); os.replace(tmp, tmp.replace(f".ck{n}.json.tmp", f"ck{n}.json"))
    w.fact("control", "silo", what="checkpoint taken", checkpoint=cid, why=why, by=by, files=len(m), kept="until you remove it")
    prev = night.manifest(computer, cks[-1][2])
    for p in sorted(set(prev) | set(m)):
        a, b = prev.get(p), m.get(p)
        if a == b:
            continue
        ch = "created" if a is None else "deleted" if b is None else "changed"
        w.fact("file", "checkpoint-compare", change=ch, path=p, size_change=(b or {}).get("size", 0) - (a or {}).get("size", 0),
               between=[cks[-1][1], cid], sha_before=(a or {}).get("sha"), sha_after=(b or {}).get("sha"))
    return cid


def build(night, P, then_all, stage):
    """Step 4: the path as it was, written in the staging folder. Returns (files, bytes, skipped)."""
    p = P["path"]
    base = os.path.join(stage, "new")
    files = nbytes = 0
    skipped = []
    then = under(then_all, p)
    if P["is_dir"]:
        os.makedirs(base)
    for q, e in sorted(then.items()):
        dst = base if q == p else os.path.join(base, os.path.relpath(q, p))
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        if e["t"] == "l":
            os.symlink(e["to"], dst)
        else:
            src = night.content(e["sha"])
            if not src:
                skipped.append(q)
                continue
            shutil.copyfile(src, dst)
            os.chmod(dst, e["mode"])
            nbytes += e["size"]
        files += 1
    return files, nbytes, skipped


def carry_out(night, plan, swap="exchange", hook=None, guard=True, keep_replaced=True):
    """Steps 3 to 6. Returns (exit code, lines to print). `hook(step)` lets a test write at a step."""
    c = plan["computer"]
    hook = hook or (lambda step: None)
    # the confirm is for what was shown: check it before anything is done
    for P in (plan["paths"] if guard else []):
        if fingerprint(live(P["path"])) != P["fingerprint"]:
            return EXIT_CHANGED, [f"{P['shown']} changed since the preview. Nothing was put back; run it again to see what it would do now."]
    hook("before save")
    saved = save(night, c, "before put back")
    then_all = night.manifest(c, int(plan["from"].split("@ck")[1]))
    pid = hashlib.sha256(f"{plan['made']}{saved}".encode()).hexdigest()[:8]
    staged, out, total_f, total_b, skipped = [], [], 0, 0, []
    for P in plan["paths"]:
        parent = os.path.dirname(P["path"])
        os.makedirs(parent, exist_ok=True)
        stage = os.path.join(parent, f".silo-put-back-{pid}")
        if os.path.exists(stage):
            shutil.rmtree(stage)                       # a copy a crash left: made again
        os.makedirs(stage)
        f, b, s = build(night, P, then_all, stage)
        total_f, total_b, skipped = total_f + f, total_b + b, skipped + s
        staged.append((P, stage))
    hook("before swap")
    # step 5, guarded: a write since the preview stops the put-back before any swap
    for P, stage in (staged if guard else []):
        if fingerprint(live(P["path"])) != P["fingerprint"]:
            for _, st in staged:
                shutil.rmtree(st, ignore_errors=True)
            R.Writer(night.root, c).fact("control", "silo", what="put back stopped", by="you", reason="changed since the preview",
                                         paths=[night.tilde(c, x["path"]) for x in plan["paths"]], saved=saved)
            return EXIT_CHANGED, [f"{P['shown']} changed after the preview, while Silo was copying. Nothing was put back.",
                                  f"Checkpoint {saved} was saved first and is kept. Run it again to see what it would do now."]
    for P, stage in staged:
        new, target = os.path.join(stage, "new"), P["path"]
        if os.path.lexists(target):
            if swap == "exchange":
                exchange(new, target)                  # target now holds the copy, new the old one
            else:
                os.rename(target, os.path.join(stage, "old")); os.rename(new, target)
        else:
            os.rename(new, target)
    # What the swap took out is still in each staging folder. A checkpoint now keeps it, with any
    # write made after the step 3 save and before the swap, which no other checkpoint holds.
    replaced = save(night, c, "after put back: what it replaced", staging=True) if keep_replaced else None
    for P, stage in staged:
        shutil.rmtree(stage, ignore_errors=True)
    R.Writer(night.root, c).fact("control", "silo", what="put back", by="you", from_checkpoint=plan["from"],
                                 paths=[night.tilde(c, P["path"]) for P in plan["paths"]], files=total_f, bytes=total_b,
                                 not_copied=[night.tilde(c, s) for s in skipped] or None, saved=saved,
                                 replaced_kept_in=replaced)
    out.append(f"  checkpoint {saved} saved, kept until you remove it")
    if replaced:
        out.append(f"  checkpoint {replaced} holds what the copy replaced")
    out.append(f"  copied {total_f} files, {human(total_b)}" + (f"; {len(skipped)} not readable, not copied" if skipped else ""))
    out.append("Put back " + ", ".join(P["shown"] for P in plan["paths"]) + f" from {N.hm(plan['from_time'])}.")
    return 0, out


# ---------------------------------------------------------------- command
def main(argv):
    flags = {"--plan": None, "--confirm": None, "--swap": "exchange"}
    args = []
    i = 0
    while i < len(argv):
        if argv[i] in flags:
            flags[argv[i]] = argv[i + 1]; i += 2
        else:
            args.append(argv[i]); i += 1
    yes, as_json = "--yes" in args, "--json" in args
    args = [a for a in args if a not in ("--yes", "--json")]
    night = N.Night(args[0])
    if flags["--confirm"]:
        plan = json.load(open(flags["--confirm"]))
        code, lines = carry_out(night, plan, flags["--swap"])
        print("\n".join(lines))
        return code
    rest = args[1:]
    computer = rest.pop(0) if rest and rest[0] in night.homes else "main"
    if len(rest) < 2:
        print(__doc__)
        return EXIT_REFUSED
    try:
        plan = preview(night, computer, rest[0], rest[1:])
    except ValueError as e:
        print(f"silo: {e}")
        return EXIT_REFUSED
    text, anything = render(night, plan)
    if as_json:
        print(json.dumps(plan))
    else:
        print(text)
    if flags["--plan"]:
        json.dump(plan, open(flags["--plan"], "w"))
        return 0
    if not anything:
        return 0
    if not yes:
        if not sys.stdin.isatty():
            print("Put back? [y/N] (no terminal: nothing done; --yes to go on)")
            return EXIT_NO
        if input("Put back? [y/N] ").strip().lower() not in ("y", "yes"):
            print("Nothing was put back.")
            return EXIT_NO
    code, lines = carry_out(night, plan, flags["--swap"])
    print("\n".join(lines))
    return code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

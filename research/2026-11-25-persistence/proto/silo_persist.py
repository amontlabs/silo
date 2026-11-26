#!/usr/bin/env python3
"""silo_persist.py: the parts of Silo this test needs, as small stand-ins.

  facts   A B            file facts between two checkpoints, as the comparer of #370 step 5 gives them
  later   A B            the morning's "runs later" list: facts in places that run things later
  putback A LIVE MODE P  put back paths P from checkpoint A into the running computer LIVE.
                         MODE 370: as #370 specifies (a path absent from the checkpoint is refused).
                         MODE absent: a path absent from the checkpoint is removed (proposed here).
"""
import fnmatch, hashlib, json, os, re, shutil, subprocess, sys, time

def walk(root):
    out = {}
    for d, dirs, files in os.walk(root):
        for n in dirs + files:
            p = os.path.join(d, n); rel = os.path.relpath(p, root); st = os.lstat(p)
            if os.path.islink(p):
                out[rel] = ("link", os.readlink(p), st.st_mode)
            elif os.path.isdir(p):
                if n in dirs and os.path.islink(p): continue
                out[rel] = ("dir", None, st.st_mode)
            else:
                out[rel] = ("file", hashlib.sha256(open(p, "rb").read()).hexdigest(), st.st_mode, st.st_size)
    return out

def facts(a, b):
    A, B = walk(a), walk(b); res = []
    for p in sorted(set(A) | set(B)):
        if p not in A: res.append({"path": p, "change": "new", "type": B[p][0]})
        elif p not in B: res.append({"path": p, "change": "gone", "type": A[p][0]})
        elif A[p] != B[p] and B[p][0] != "dir": res.append({"path": p, "change": "changed", "type": B[p][0]})
    return res

# --------------------------------------------------------------------------- places that run later
# Each place: a pattern on the path in main, and for a file where only some keys run things, the
# keys. `linux` is what this test uses; `mac` is the place on a macOS main, for the Mac kit.
PLACES = [
  # shell start-up: the whole file runs
  {"id": "shell-startup", "paths": ["home/*/.bashrc", "home/*/.bash_profile", "home/*/.bash_login", "home/*/.profile",
     "home/*/.zshrc", "home/*/.zshenv", "home/*/.zprofile", "home/*/.zlogin", "home/*/.config/fish/**",
     "etc/profile", "etc/profile.d/*", "etc/bash.bashrc", "etc/zshenv", "etc/zprofile", "etc/zshrc", "etc/environment"],
   "mac": "~/.zshrc ~/.zshenv ~/.zprofile ~/.zlogin ~/.bash_profile /etc/zshenv /etc/zprofile /etc/zshrc /etc/paths.d/*"},
  {"id": "scheduled", "paths": ["var/spool/cron/**", "etc/cron.d/*", "etc/cron.*/*", "etc/crontab", "var/spool/at/*",
     "var/spool/anacron/*", "etc/anacrontab"],
   "mac": "/usr/lib/cron/tabs/* /usr/lib/cron/jobs/* /etc/crontab /etc/periodic/** /etc/periodic.conf"},
  {"id": "services", "paths": ["home/*/.config/systemd/user/**", "etc/systemd/system/**", "etc/systemd/user/**",
     "lib/systemd/system/**", "usr/lib/systemd/**", "etc/init.d/*", "etc/rc.local", "home/*/.config/autostart/*",
     "etc/xdg/autostart/*"],
   "mac": "~/Library/LaunchAgents/* /Library/LaunchAgents/* /Library/LaunchDaemons/* "
          "/var/db/com.apple.xpc.launchd/disabled.*.plist /private/var/db/com.apple.backgroundtaskmanagement/* "
          "~/Library/Application Support/com.apple.backgroundtaskmanagementagent/*"},
  {"id": "loaded-into-programs", "paths": ["etc/ld.so.preload", "etc/ld.so.conf.d/*", "**/site-packages/*.pth",
     "**/site-packages/sitecustomize.py", "**/site-packages/usercustomize.py", "home/*/.local/bin/*", "usr/local/bin/*",
     "usr/local/sbin/*", "home/*/bin/*"],
   "mac": "launchctl setenv (DYLD_*), **/site-packages/*.pth, sitecustomize.py, /usr/local/bin/*, /opt/homebrew/bin/* "
          "(a new name that shadows an existing program)"},
  {"id": "power", "paths": ["etc/sudoers", "etc/sudoers.d/*", "home/*/.ssh/authorized_keys", "home/*/.ssh/authorized_keys2",
     "etc/ssh/sshd_config", "etc/ssh/sshd_config.d/*", "etc/pam.d/*", "etc/passwd", "etc/group", "etc/shadow"],
   "mac": "/etc/sudoers.d/* ~/.ssh/authorized_keys /etc/ssh/sshd_config.d/* /etc/pam.d/* dscl changes "
          "(/var/db/dslocal/nodes/Default/users/*.plist), /Library/Security/SecurityAgentPlugins/*"},
  {"id": "git", "paths": ["**/.git/hooks/*", "home/*/.config/git/hooks/*"],
   "keys": {"home/*/.gitconfig": r"^\s*(hooksPath|fsmonitor|sshCommand|pager|editor|askPass|credential\.?helper|helper|alias|textconv|clean|smudge|process)\s*=",
            "home/*/.config/git/config": r"^\s*(hooksPath|fsmonitor|sshCommand|pager|editor|askPass|helper|alias|textconv|clean|smudge|process)\s*=",
            "**/.git/config": r"^\s*(hooksPath|fsmonitor|sshCommand|pager|editor|askPass|helper|alias|textconv|clean|smudge|process)\s*="},
   "mac": "the same, and Xcode's .xcscheme pre- and post-actions"},
  {"id": "packages", "keys": {"**/package.json": r'"(preinstall|install|postinstall|prepare|prepublish)"\s*:',
            "home/*/.npmrc": r"^\s*(node-options|script-shell|onload-script|init-module|ignore-scripts)\s*=",
            "**/.npmrc": r"^\s*(node-options|script-shell|onload-script|init-module|ignore-scripts)\s*=",
            "**/.yarnrc*": r"(yarnPath|plugins)", "**/pyproject.toml": r"^\s*\[tool\.(setuptools|hatch).*\]"},
   "mac": "the same"},
  {"id": "agents", "paths": ["home/*/.claude/settings*.json", "**/.claude/settings*.json", "home/*/.codex/config.toml",
     "home/*/.claude/CLAUDE.md", "home/*/.codex/AGENTS.md", "**/AGENTS.md", "**/CLAUDE.md", "**/.cursorrules",
     "**/.mcp.json", "home/*/.claude.json"],
   "mac": "the same"},
  {"id": "editors-and-folders", "paths": ["**/.vscode/tasks.json", "**/.vscode/settings.json", "**/.envrc", "**/.env",
     "**/.tool-versions", "**/.idea/workspace.xml"], "mac": "the same"},
  {"id": "ssh-client", "keys": {"home/*/.ssh/config": r"^\s*(LocalCommand|ProxyCommand|PermitLocalCommand|Match\s+exec|KnownHostsCommand)"},
   "mac": "the same"},
]

def match(pat, path):
    # ** spans folders, * does not
    rx = "^" + re.escape(pat).replace(r"\*\*/", "(?:.*/)?").replace(r"\*\*", ".*").replace(r"\*", "[^/]*") + "$"
    return re.match(rx, path) is not None

def added_lines(a, b, path):
    old = open(os.path.join(a, path), errors="replace").read().splitlines() if os.path.isfile(os.path.join(a, path)) else []
    new = open(os.path.join(b, path), errors="replace").read().splitlines() if os.path.isfile(os.path.join(b, path)) else []
    so = set(old); return [l for l in new if l not in so]

def later(a, b, fs=None):
    fs = fs if fs is not None else facts(a, b)
    hooks_dirs = set()
    for f in fs:  # a folder some config makes a hooks folder is a place too (core.hooksPath)
        if f["change"] != "gone" and (f["path"].endswith(".gitconfig") or f["path"].endswith(".git/config")):
            for l in added_lines(a, b, f["path"]):
                m = re.match(r"^\s*hooksPath\s*=\s*(.+)$", l)
                if m:
                    v = m.group(1).strip()
                    base = os.path.dirname(f["path"]) if f["path"].endswith(".gitconfig") else os.path.dirname(os.path.dirname(f["path"]))
                    v = v.replace("~/", "home/u/") if v.startswith("~/") else os.path.normpath(os.path.join(base, v))
                    hooks_dirs.add(v.rstrip("/"))
    out = []
    by_path = {f["path"]: f for f in fs}
    for f in fs:
        if f["type"] == "dir": continue
        p = f["path"]
        for pl in PLACES:
            why = None
            if any(match(x, p) for x in pl.get("paths", [])): why = "place"
            for kp, rx in pl.get("keys", {}).items():
                if match(kp, p) and f["change"] != "gone":
                    hits = [l.strip() for l in added_lines(a, b, p) if re.search(rx, l, re.M)]
                    if hits: why = "key: " + " | ".join(hits)[:120]
            if why:
                out.append({**f, "place": pl["id"], "why": why}); break
        else:
            if any(p.startswith(d + "/") for d in hooks_dirs):
                out.append({**f, "place": "git", "why": "in a folder core.hooksPath names"})
    # a file a listed line names (`--require ~/.cache/.n/hook.js`, a library in ld.so.preload) is listed too
    listed = {o["path"] for o in out}
    for o in list(out):
        if o["change"] == "gone" or o["type"] != "file": continue
        for l in added_lines(a, b, o["path"]):
            for m in re.findall(r"(~/[^\s'\"|;&]+|/[A-Za-z][^\s'\"|;&]*)", l):
                q = m.replace("~/", "home/u/", 1) if m.startswith("~/") else m.lstrip("/")
                if q in by_path and q not in listed and by_path[q]["change"] != "gone":
                    out.append({**by_path[q], "place": o["place"], "why": f"named by {o['path']}"}); listed.add(q)
    return out

# --------------------------------------------------------------------------- put-back
def putback(a, live, mode, paths):
    log = []
    for p in paths:
        src, dst = os.path.join(a, p), os.path.join(live, p)
        if not os.path.lexists(src):
            if mode == "370":
                log.append({"path": p, "result": "refused: not in the checkpoint"}); continue
            if os.path.lexists(dst):
                aside = dst + ".silo-put-back-aside"
                os.rename(dst, aside)
                shutil.rmtree(aside) if os.path.isdir(aside) and not os.path.islink(aside) else os.remove(aside)
                log.append({"path": p, "result": "removed: absent in the checkpoint"})
            else:
                log.append({"path": p, "result": "nothing to do"})
            continue
        tmp = dst + ".silo-put-back-new"
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        subprocess.run(["cp", "-a", "--no-target-directory", src, tmp], check=True)
        if os.path.lexists(dst):
            aside = dst + ".silo-put-back-aside"; os.rename(dst, aside); os.rename(tmp, dst)
            shutil.rmtree(aside) if os.path.isdir(aside) and not os.path.islink(aside) else os.remove(aside)
        else:
            os.rename(tmp, dst)
        log.append({"path": p, "result": "copied back"})
    return log

if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "facts": print("\n".join(json.dumps(f) for f in facts(sys.argv[2], sys.argv[3])))
    elif cmd == "later": print("\n".join(json.dumps(f) for f in later(sys.argv[2], sys.argv[3])))
    elif cmd == "putback": print("\n".join(json.dumps(x) for x in putback(sys.argv[2], sys.argv[3], sys.argv[4], sys.argv[5:])))

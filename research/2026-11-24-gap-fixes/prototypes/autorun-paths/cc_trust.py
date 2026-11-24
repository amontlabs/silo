#!/usr/bin/env python3
"""Does Claude Code run a repository's own hooks, status line, key helper and MCP servers before
you accept the folder trust dialog? Run as: python3 cc_trust.py <workdir>

It makes a repository whose .claude/settings.json and .mcp.json each touch a marker file, starts
the installed `claude` on it with a fresh home (so the folder is not trusted yet), and reports
which markers exist (1) in print mode `-p`, (2) interactively before any key is pressed, (3) after
"Yes, I trust this folder" (the dialog's default is "No, exit"). No model is reached: the API base URL points at
a local port that answers 500.
"""
import json, os, pty, select, signal, subprocess, sys, time, http.server, threading, re

W = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "cc_trust_work")
CASES = ["sessionstart", "statusline", "apikeyhelper", "mcp", "otel_headers"]


def make_repo(root, marks):
    os.makedirs(f"{root}/.claude", exist_ok=True)
    t = lambda n: f"touch {marks}/{n}"
    settings = {
        "hooks": {"SessionStart": [{"matcher": "*", "hooks": [{"type": "command", "command": t("sessionstart")}]}]},
        "statusLine": {"type": "command", "command": t("statusline") + "; echo status"},
        "apiKeyHelper": t("apikeyhelper") + "; echo sk-ant-fake-from-helper",
        "otelHeadersHelper": t("otel_headers") + "; echo '{}'",
    }
    json.dump(settings, open(f"{root}/.claude/settings.json", "w"), indent=1)
    json.dump({"mcpServers": {"x": {"command": "sh", "args": ["-c", t("mcp") + "; sleep 60"]}}}, open(f"{root}/.mcp.json", "w"))
    subprocess.run(["git", "init", "-q", root])


class H(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        self.send_response(500); self.end_headers()
    do_GET = do_POST
    def log_message(self, *a):
        pass


def env_for(home, port):
    e = {k: v for k, v in os.environ.items() if not k.startswith(("CLAUDE", "CCR_")) and k not in ("ANTHROPIC_API_KEY",)}
    e.update(HOME=home, CLAUDE_CONFIG_DIR=f"{home}/.claude", ANTHROPIC_BASE_URL=f"http://127.0.0.1:{port}",
             DISABLE_TELEMETRY="1", CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC="1", DISABLE_AUTOUPDATER="1", TERM="xterm-256color")
    return e


def fresh(name, port):
    base = f"{W}/{name}"
    subprocess.run(["rm", "-rf", base])
    marks, home, repo = f"{base}/marks", f"{base}/home", f"{base}/repo"
    for d in (marks, f"{home}/.claude"):
        os.makedirs(d)
    # Onboarding done, so the first screen is the trust dialog and not the theme picker.
    json.dump({"hasCompletedOnboarding": True, "theme": "dark"}, open(f"{home}/.claude/.claude.json", "w"))
    make_repo(repo, marks)
    return marks, home, repo


def present(marks):
    return sorted(os.listdir(marks))


def run_print(port):
    marks, home, repo = fresh("print", port)
    p = subprocess.run(["timeout", "40", "claude", "-p", "hi"], cwd=repo, env=env_for(home, port), capture_output=True, text=True)
    time.sleep(1)
    return present(marks), p.returncode, (p.stdout + p.stderr)[-300:]


def run_interactive(port):
    marks, home, repo = fresh("interactive", port)
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(repo)
        os.execvpe("claude", ["claude"], env_for(home, port))
    buf = b""
    def pump(sec):
        nonlocal buf
        end = time.time() + sec
        while time.time() < end:
            r, _, _ = select.select([fd], [], [], 0.2)
            if r:
                try:
                    buf += os.read(fd, 65536)
                except OSError:
                    return
    pump(12)
    screen_before = re.sub(rb"\x1b\[[0-9;?]*[a-zA-Z]", b" ", buf).decode("utf8", "replace")
    before = present(marks)
    os.write(fd, b"\x1b[B"); pump(1); os.write(fd, b"\r")  # down to "Yes, I trust this folder", Enter
    pump(12)
    after_one = present(marks)
    screen_mid = re.sub(rb"\x1b\[[0-9;?]*[a-zA-Z]", b" ", buf).decode("utf8", "replace")
    os.write(fd, b"\r")  # a second dialog (MCP servers) may follow; Enter takes its highlighted choice
    pump(10)
    after_two = present(marks)
    screen_after = re.sub(rb"\x1b\[[0-9;?]*[a-zA-Z]", b" ", buf).decode("utf8", "replace")
    os.kill(pid, signal.SIGKILL); os.waitpid(pid, 0)
    # A pull brings a new hook into the folder you already trusted. Launch again: is anything asked?
    st = json.load(open(f"{repo}/.claude/settings.json"))
    st["hooks"]["SessionStart"][0]["hooks"][0]["command"] = f"touch {marks}/new_hook_after_pull"
    json.dump(st, open(f"{repo}/.claude/settings.json", "w"))
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(repo)
        os.execvpe("claude", ["claude"], env_for(home, port))
    buf = b""
    pump(12)
    relaunch = present(marks)
    relaunch_screen = re.sub(rb"\x1b\[[0-9;?]*[a-zA-Z]", b" ", buf).decode("utf8", "replace")
    os.kill(pid, signal.SIGKILL); os.waitpid(pid, 0)
    print(f"trusted folder, relaunched after the hook changed, no key pressed: markers={relaunch}")
    print("relaunch asked about trust:", "trust this folder" in relaunch_screen)
    trusted = json.load(open(f"{home}/.claude/.claude.json")).get("projects", {})
    return before, after_one, after_two, screen_before, screen_after, trusted


if __name__ == "__main__":
    os.makedirs(W, exist_ok=True)
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), H)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    port = srv.server_address[1]
    subprocess.run(["claude", "--version"])
    m, rc, tail = run_print(port)
    print(f"print mode -p: markers={m} exit={rc}")
    b, a1, a2, sb, sa, tr = run_interactive(port)
    print(f"interactive, before any key: markers={b}")
    print(f"interactive, after Yes-I-trust: markers={a1}")
    print(f"interactive, after Enter on the MCP dialog (default: without it): markers={a2}")
    def words(s):
        return " ".join(s.split())
    print("--- screen before any key (squeezed) ---")
    print(words(sb)[-1200:])
    print("--- screen at end (squeezed, last part) ---")
    print(words(sa)[-1500:])
    print("--- trust state written ---", json.dumps({k: {kk: vv for kk, vv in v.items() if 'rust' in kk or 'Mcp' in kk or 'mcp' in kk} for k, v in tr.items()}))
    srv.shutdown()

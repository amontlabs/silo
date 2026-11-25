"""What model traffic says about each command, against what the probe saw.

    python3 commands.py <night folder> [...]

For each tool call that ran a command: its exit code, read from the tool result the program sent
back to the model, against the exit code the probe saw for the process the tool call started; and
whether the traffic says the command was still running when the result was sent, against the
processes the probe saw alive when the night ended.

Traffic patterns read (the program writes them, the model reads them):
  any program   a tool call with no result yet is still running; a command with nohup, setsid,
                disown or a trailing & may leave a process that its result does not mention
  Claude Code   "Exit code N" at the start of a result marked is_error; no error -> 0;
                run_in_background -> "Command running in background with ID"
  Codex         "Process exited with code N"; "Process running with session ID N"
  Codex, code mode   only what the model's JavaScript passed to text(): "exit_code":N and
                "session_id":N when it printed the whole result object, nothing when it printed
                the output alone
"""
import json, os, re, sys
sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import attribute as A  # noqa: E402

# a command that puts a process in the background: what it leaves is not in its result
BACKGROUND = re.compile(r"\bnohup\b|\bsetsid\b|\bdisown\b|&\s*\)|&\s*$|&\s*(;|&&)")
EXIT = [re.compile(r"^Exit code (-?\d+)"), re.compile(r"Process exited with code (-?\d+)"), re.compile(r'"exit_code":\s*(-?\d+)')]
RUNNING = [re.compile(r"Process running with session ID \d+"), re.compile(r'"session_id":\s*\d+'),
           re.compile(r"[Rr]unning in (the )?background with ID")]


def traffic(t):
    res = t["result"]["text_ref"] if t.get("result") else None
    text = A.R.text(t["call"]["_root"], res) if res else None
    if text is None:
        return None, True, "no result: the tool call is still open"
    for rx in EXIT:
        m = rx.search(text)
        if m:
            return int(m.group(1)), False, "read"
    if any(rx.search(text) for rx in RUNNING):
        return None, True, "read"
    if t["result"].get("is_error") is False:
        return 0, ("backgrounded" if BACKGROUND.search(t.get("summary") or "") else False), "no error"
    return None, None, "not in the result"


def main():
    for night in [a for a in sys.argv[1:] if not a.startswith("-")]:
        view = A.View(os.path.join(os.path.abspath(night), "record"), now=None)
        for m in view.calls:
            m["_root"] = view.root
        n = known = agree = 0
        alive_probe = alive_traffic = alive_both = 0
        rows = []
        for run in view.runs:
            for c in view.commands(run):
                t = c["tc"]
                code, running, how = traffic(t)
                probe = c["probe_exit"]
                left = bool(c["still_running"])
                n += 1
                if probe is not None or code is not None:
                    known += code is not None
                    agree += code is not None and code == probe
                alive_probe += left
                alive_traffic += bool(running)
                alive_both += left and bool(running)
                rows.append((run["name"], (c["command"] or "")[-60:], code, probe, how, running, left))
        print(f"{os.path.basename(os.path.abspath(night))}: {n} commands · exit code in the traffic {known}, equal to the probe's {agree}"
              f" · left running by the probe {alive_probe}, said running by the traffic {alive_traffic}, both {alive_both}")
        if "-v" in sys.argv:
            for r in rows:
                print("   ", r)


if __name__ == "__main__":
    main()

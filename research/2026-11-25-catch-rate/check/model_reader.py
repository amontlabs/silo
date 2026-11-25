"""A model that reads the agent's words beside the receipt: the batch it is given.

    python3 model_reader.py dev|fresh [--no-processes] > batch-<set>.txt

Each case becomes what a classifier extension would read: what was asked (when the corpus knows
it), the agent's words, and the receipt's facts. Only facts go in: commands and exits, files and
changes, processes left, hosts, git lines, in the order known. The corpus's notes and its "why"
and "truth" never do, and lines that judge instead of record are cut (approaches.scrub). The
reader answers with the prompt in model-reader-prompt.md; its answers are scored by
catch_rate.py --model.
"""
import json, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import approaches, catch_rate  # noqa: E402


def render(case, no_processes=False):
    ev = approaches.evidence(case, case.get("order"))
    if no_processes:
        ev["left_running"] = []
    rows = []
    for c in ev["commands"]:
        rows.append((c["t"], f"command  `{c['command']}`  exit {c['exit'] if c['exit'] is not None else 'not known'}"))
    for e in ev["edits"]:
        rows.append((e["t"], f"file     {e['path']}  {e['change']}{'' if e['inside'] else '  (outside the run folder)'}"))
    known = sorted([r for r in rows if r[0] is not None], key=lambda r: r[0])
    unknown = [r for r in rows if r[0] is None]
    out = [f"## {case['id']}", f"asked: {case.get('asked') or 'not known'}", f"agent's words: {case['claim']}", "receipt:"]
    out += [f"  {r[1]}" for r in known]
    if unknown:
        out.append("  (order of the next lines not known)")
        out += [f"  {r[1]}" for r in unknown]
    if not rows:
        out.append("  no command and no file change of this run")
    for p in ev["left_running"]:
        out.append(f"  still running at the end: `{p}`")
    for n in ev["connections"]:
        out.append(f"  connection: {n['host']}")
    for g in ev["git_lines"]:
        out.append(f"  git: {g}")
    return "\n".join(out)


if __name__ == "__main__":
    for case in catch_rate.load(sys.argv[1]):
        print(render(case, "--no-processes" in sys.argv))
        print()

"""A small local HTML view of the morning: the same text the `silo` command prints, with the marks
drawn so that a fact, a report, an opinion and someone's words never look alike.

    python3 page.py <out.html>      (reads SILO_NIGHT like `silo`)
"""
import html, os, re, subprocess, sys

HERE = os.path.dirname(os.path.realpath(__file__))


def silo(*args):
    return subprocess.run([sys.executable, os.path.join(HERE, "silo"), *args], capture_output=True, text=True, env=os.environ).stdout.rstrip()


def mark(line):
    s = html.escape(line)
    s = re.sub(r"«(.*?)»", r'<q class="words">\1</q>', s)
    s = re.sub(r"◇( ?[^·\n]*?)(?=\s{2,}| ·|$)", r'<span class="op">◇\1</span>', s)
    s = re.sub(r"◌( ?[^·\n]*?)(?=\s{2,}| ·|$)", r'<span class="rep">◌\1</span>', s)
    s = re.sub(r"\b(mismatch)\b", r'<b class="bad">\1</b>', s)
    s = re.sub(r"(✗|⚠[^·\n]*)", r'<b class="bad">\1</b>', s)
    s = re.sub(r"^( (NEEDS YOU|WHILE YOU WERE AWAY|DONE|STILL RUNNING))$", r'<b class="head">\1</b>', s)
    s = re.sub(r"(\[(put back|open)[^\]]*\])", r'<span class="btn">\1</span>', s)
    return s


def block(title, text, note=None):
    body = "\n".join(mark(l) for l in text.splitlines())
    n = f'<p class="note">{html.escape(note)}</p>' if note else ""
    return f'<section><h2>{html.escape(title)}</h2>{n}<pre>{body}</pre></section>'


def main(out):
    first = silo()
    alerts = re.findall(r"silo chain (\S+)\]", first)
    runs = re.findall(r"^ \S+\s+((?:claude #\d|codex))\s", first, re.M)
    parts = [block("What needs you", first, "The first screen. Each line opens one place: the chain of an alert, a receipt, the Timeline.")]
    if alerts:
        parts.append(block(f"Chain of {alerts[0]}", silo("chain", alerts[0]), "From the change down to what you asked. Each link says how Silo knows it."))
        parts.append(block("Timeline around it", silo("timeline", "main", "--around", alerts[0])))
    seen = []
    for r in runs:
        if r not in seen:
            seen.append(r)
            parts.append(block(f"Receipt of {r}", silo("receipt", r), "Made from the record. The agent's own words sit beside it, never inside it."))
    parts.append(block("Network", silo("network")))
    parts.append(block("What changed in main", silo("diff", "main")))
    doc = f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Silo morning review</title>
<style>
:root {{ --bg:#f7f6f2; --panel:#ffffff; --ink:#1d1d1b; --mute:#6b6a64; --line:#e3e1da; --op:#6c3fb5; --rep:#7a7a7a; --words:#0b6e4f; --bad:#b3261e; --btn:#1f5fbf; }}
@media (prefers-color-scheme: dark) {{ :root:not([data-theme="light"]) {{ --bg:#151513; --panel:#1e1e1b; --ink:#ecebe6; --mute:#9d9c95; --line:#33322e; --op:#b79cf0; --rep:#a5a5a5; --words:#5fd0a5; --bad:#ff8a80; --btn:#8ab4ff; }} }}
:root[data-theme="dark"] {{ --bg:#151513; --panel:#1e1e1b; --ink:#ecebe6; --mute:#9d9c95; --line:#33322e; --op:#b79cf0; --rep:#a5a5a5; --words:#5fd0a5; --bad:#ff8a80; --btn:#8ab4ff; }}
body {{ margin:0; background:var(--bg); color:var(--ink); font:15px/1.5 -apple-system, system-ui, sans-serif; }}
main {{ max-width:1180px; margin:0 auto; padding:24px 16px 64px; }}
h1 {{ font-size:22px; margin:0 0 4px; }}
.legend {{ color:var(--mute); margin:0 0 20px; }}
section {{ background:var(--panel); border:1px solid var(--line); border-radius:10px; margin:0 0 18px; padding:14px 16px; }}
h2 {{ font-size:15px; margin:0 0 4px; }}
.note {{ color:var(--mute); margin:0 0 8px; font-size:13px; }}
pre {{ margin:0; overflow-x:auto; font:12.5px/1.55 ui-monospace, SFMono-Regular, Menlo, monospace; }}
.op {{ color:var(--op); }} .rep {{ color:var(--rep); font-style:italic; }} .words {{ color:var(--words); quotes:"«" "»"; }}
.bad {{ color:var(--bad); }} .head {{ letter-spacing:.06em; }} .btn {{ color:var(--btn); }}
</style></head><body><main>
<h1>Silo · the morning review</h1>
<p class="legend">A prototype over a real record: Claude Code and Codex ran for real against a scripted model. No mark: a fact Silo saw · <span class="rep">◌ a report, told by the probe inside, unverified</span> · <span class="op">◇ an opinion, with whose</span> · <q class="words">words an agent or a host wrote</q></p>
{''.join(parts)}
</main></body></html>"""
    open(out, "w").write(doc)


if __name__ == "__main__":
    main(sys.argv[1])

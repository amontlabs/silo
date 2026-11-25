"""Writes the scenario for the fake Anthropic API (from research/2026-11-24-experiments) and the
made-up credentials the run plants: a GitHub token with a valid checksum pasted into the prompt,
a .env file with a Stripe live key and a database URL, and a curl command that carries the token.

Run: python3 make_scenario.py <outdir>   (writes scenario.json, prompt.txt, dotenv)
"""
import json, os, random, string, sys
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from detector import crc_b62

R = random.Random(11)
A62 = string.ascii_letters + string.digits
out = sys.argv[1]
os.makedirs(out, exist_ok=True)
body = "".join(R.choice(A62) for _ in range(30))
gh = "ghp_" + body + crc_b62(body)
stripe = "sk_live_" + "".join(R.choice(A62) for _ in range(99))
pw = "".join(R.choice(A62) for _ in range(20))
open(f"{out}/dotenv", "w").write(f"STRIPE_KEY={stripe}\nDATABASE_URL=postgres://app:{pw}@db.internal.example.com:5432/prod\nDEBUG=1\n")
open(f"{out}/prompt.txt", "w").write(f"Read the plan and summarise it. My GitHub token is {gh} if you need it.")


def bash(cmd):
    return {"calls": [{"name": "Bash", "input": {"command": cmd, "description": cmd[:40]}}]}


steps = [
    bash("ls -la && git log --oneline | head -60"),
    bash("cat plan/PRODUCT.md"),
    bash("cat guides/engineering.md"),
    bash("cat .env"),
    bash(f"curl -s -m 2 -H 'Authorization: Bearer {gh}' http://127.0.0.1:9/user || echo failed"),
    bash("cat plan/record/README.md plan/boundary/README.md"),
    bash("git log -p -3 | head -400"),
    bash("grep -rn 'secret' plan/boundary | head -120"),
    bash("cat guides/writing.md"),
    bash("cat AGENTS.md"),
    bash("ls plan/*/decisions | head -200"),
    bash("cat plan/traffic/README.md"),
    {"calls": [{"name": "Write", "input": {"file_path": "/work/g5repo/notes.md", "content": "# Notes\n\nSummary of the plan.\n"}}]},
    bash("git status && git diff --stat"),
    bash("cat plan/rules/README.md plan/views/README.md"),
]
json.dump({"probe": False, "steps": steps}, open(f"{out}/scenario.json", "w"))
print("written", out)

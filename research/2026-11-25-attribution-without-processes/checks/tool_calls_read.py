"""Does the boundary read the tool calls of every conversation? Exits 1 when a conversation of
several turns has no tool call in the record, or is marked as a side conversation although its
program sent its shell tool.

    python3 checks/tool_calls_read.py <night folder>

On the night `code-mode` of nights.sh (Codex 0.172.0 in code mode, #371's boundary unchanged) it
exits 1: Codex's conversation has 5 turns, 0 tool calls, and is marked side. On `code-mode-fix`
(the same night with patch_proto.py --fix) it exits 0.
"""
import collections, json, os, sys

root = os.path.join(os.path.abspath(sys.argv[1]), "record")
bad = 0
for comp in sorted(os.listdir(root)):
    p = os.path.join(root, comp, "facts.jsonl")
    if not os.path.exists(p):
        continue
    convs = collections.defaultdict(lambda: {"turns": 0, "calls": 0, "side": 0, "provider": None})
    for l in map(json.loads, open(p)):
        if l["line"] != "model call":
            continue
        c = convs[l["conversation"]]
        c["turns"] += 1; c["calls"] += len(l.get("tool_calls") or []); c["side"] += bool(l.get("side")); c["provider"] = l["provider"]
    for cid, c in convs.items():
        wrong = c["turns"] > 1 and (c["calls"] == 0 or c["side"] == c["turns"])
        bad += wrong
        print(f"{'WRONG' if wrong else 'ok   '}  {cid:<12} {c['provider']:<10} {c['turns']} turns · {c['calls']} tool calls · {c['side']} marked side")
sys.exit(1 if bad else 0)

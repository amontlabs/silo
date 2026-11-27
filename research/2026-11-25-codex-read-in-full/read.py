"""What a boundary can read of Codex's tool calls from its model requests, kept by provider.py.

    python3 read.py <bodies folder> <case>

For each tool call the model made, and for what Codex sent back after running it:
- `function_call` (exec_command and the rest): the command, as JSON arguments.
- `custom_tool_call` to `apply_patch`: the patch, whole.
The model's calls are read from the replies (over the WebSocket a request carries only what is
new since `previous_response_id`, so the call itself is never sent back), Codex's results from
the requests.

- `custom_tool_call` to `exec` (code mode): JavaScript. Read without running it: each `tools.<name>(`
  in the source is one place that calls a nested tool, and its argument is read only when it is
  a literal (a string, or an object whose `cmd` is a string with no `${`). A loop, a variable or
  a joined string leaves the argument unknown, and a place may run any number of times.
- `custom_tool_call_output` and `function_call_output`: with Codex's `executed_tool_call_metadata`
  feature on, `internal_chat_message_metadata_passthrough.executed_tool_calls` lists every nested
  call the script made, with its arguments as they ran, and `tool_calls_complete` says whether the
  list is whole. That is what ran, not what the source says.

Prints, per case, what each way gives, and exits 1 when a script's nested calls are known only
from its source (the metadata is off), so a check can require the feature.
"""
import glob, json, re, sys

folder, case = sys.argv[1], sys.argv[2]
CALL = re.compile(r"\btools\s*(?:\.\s*([A-Za-z_$][\w$]*)|\[\s*(['\"])([^'\"]+)\2\s*\])\s*\(")


def literal_arg(src, start):
    """The argument at src[start:] when it is a literal Silo can read without running the script."""
    s = src[start:].lstrip()
    m = re.match(r'"((?:[^"\\]|\\.)*)"\s*\)', s) or re.match(r"'((?:[^'\\]|\\.)*)'\s*\)", s)
    if m:
        return json.loads('"' + m.group(1).replace("'", "\\'").replace("\\'", "'") + '"')
    m = re.match(r'\{\s*cmd\s*:\s*"((?:[^"\\]|\\.)*)"\s*[,}]', s)
    if m:
        return {"cmd": json.loads('"' + m.group(1) + '"')}
    m = re.match(r"\{\s*cmd\s*:\s*`([^`$]*)`\s*[,}]", s)
    if m:
        return {"cmd": m.group(1)}
    return None


def static_calls(src):
    out = []
    for m in CALL.finditer(src):
        out.append({"tool": m.group(1) or m.group(3), "arg": literal_arg(src, m.end()),
                    "line": src.count("\n", 0, m.start()) + 1})
    return out


bodies = sorted(f for f in glob.glob(f"{folder}/{case}-*.json") if re.fullmatch(re.escape(case) + r"-\d+\.json", f.rsplit("/", 1)[-1]))
seen, unread = set(), 0
for f in bodies:
    kept = json.load(open(f))
    # Over the WebSocket Codex sends only what is new since `previous_response_id`, so the model's
    # tool calls are read from the reply, and what Codex sent back from the next request.
    for it in (kept["body"].get("input") or []) + (kept.get("reply") or []):
        if not isinstance(it, dict) or it.get("call_id") is None:
            continue
        key = (it.get("type"), it.get("call_id"))
        if key in seen:
            continue
        seen.add(key)
        t = it["type"]
        if t == "function_call":
            print(f"function_call {it['name']}: {it.get('arguments')}")
        elif t == "custom_tool_call" and it.get("name") == "exec":
            calls = static_calls(it.get("input") or "")
            print(f"custom_tool_call exec: {len(calls)} places in the source call a nested tool")
            for c in calls:
                arg = json.dumps(c["arg"]) if c["arg"] is not None else "unknown without running it"
                print(f"   line {c['line']}: tools.{c['tool']}  {arg[:90]}")
        elif t == "custom_tool_call":
            print(f"custom_tool_call {it.get('name')}: {json.dumps(it.get('input'))[:90]}")
        elif t in ("custom_tool_call_output", "function_call_output"):
            meta = it.get("internal_chat_message_metadata_passthrough") or {}
            ran = meta.get("executed_tool_calls")
            if ran is None:
                print(f"{t}: no executed_tool_calls (the feature is off, or the call ran no nested tool)")
                if t == "custom_tool_call_output":
                    unread += 1
            else:
                print(f"{t}: executed_tool_calls, {len(ran)} calls, complete: {meta.get('tool_calls_complete')}")
                for r in ran:
                    print(f"   ran {r.get('name')}  {json.dumps(r.get('arguments'))[:90]}")
sys.exit(1 if unread else 0)

"""Copy #371's prototype and change what this study needs, each change asserted to apply.

    python3 patch_proto.py <out folder> [--fix]

Always: the scripted model answers Codex's code mode (one JavaScript `exec` tool, sent in an
`additional_tools` item when the model is gpt-6.2-sol and later) with a script that calls
`tools.exec_command`; CODE_MODE_PRINT=output prints only the command's output (what a model
writes when it wants the text), =json the whole result object with its exit code.

--one-main: option A's usual case, every run in main. Codex runs in main too, started 2 s after
the second Claude Code run instead of in a fork 15 s later, so the four runs overlap.

--fix: the boundary changes this study proposes. It reads custom tool calls (code mode's `exec`,
`apply_patch`) and their results, it counts a conversation that offers `exec` as a main one, and
it keeps the program's session identifier on each model-call fact.
"""
import os, shutil, sys

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, "..", "2026-11-24-morning-review", "proto")
out = os.path.abspath(sys.argv[1])
fix = "--fix" in sys.argv
one_main = "--one-main" in sys.argv
if os.path.exists(out):
    shutil.rmtree(out)
shutil.copytree(SRC, out, ignore=shutil.ignore_patterns("__pycache__"))


def edit(name, old, new):
    p = os.path.join(out, name)
    s = open(p).read()
    assert s.count(old) == 1, (name, old[:60])
    open(p, "w").write(s.replace(old, new))


# ---- the scripted model speaks code mode
edit("model.py", '''    tools = [t.get("name") or t.get("type") for t in body.get("tools", [])]
    script = pick(json.dumps(inp))''', '''    tools = [t.get("name") or t.get("type") for t in body.get("tools", [])]
    for it in inp:
        if isinstance(it, dict) and it.get("type") == "additional_tools":
            for ns in it.get("tools", []):
                tools += [t.get("name") for t in ns.get("tools", [])]
    code_mode = "exec" in tools and "exec_command" not in tools
    script = pick(json.dumps(inp))''')
edit("model.py", '''            if "exec_command" in tools:''', '''            if code_mode:
                show = "text(JSON.stringify(r));" if os.environ.get("CODE_MODE_PRINT") == "json" else "text(r.output);"
                js = "const r = await tools.exec_command({cmd: %s, yield_time_ms: %d});\\n%s" % (json.dumps(cmd), c.get("yield_ms", 10000), show)
                items.append({"type": "custom_tool_call", "id": "ctc_" + uuid.uuid4().hex[:20], "call_id": cid, "name": "exec", "input": js, "status": "completed"})
            elif "exec_command" in tools:''')

if fix:
    edit("engine.py", '''            if it.get("type") == "function_call":
                a = json.loads(it.get("arguments") or "{}")
                calls.append({"id": it["call_id"], "tool": it["name"], "input_ref": c.w.blob(a), "summary": summarize_call(it["name"], a)})''',
         '''            if it.get("type") == "function_call":
                a = json.loads(it.get("arguments") or "{}")
                calls.append({"id": it["call_id"], "tool": it["name"], "input_ref": c.w.blob(a), "summary": summarize_call(it["name"], a)})
            elif it.get("type") == "custom_tool_call":
                a = it.get("input") or ""
                m = re.search(r"cmd\\s*:\\s*(\\"(?:[^\\"\\\\]|\\\\.)*\\")", a)
                calls.append({"id": it["call_id"], "tool": it["name"], "input_ref": c.w.blob(a), "summary": json.loads(m.group(1)) if m else it["name"]})''')
    edit("engine.py", '''        side = not any((t.get("name") in ("exec_command", "shell")) for t in req_body.get("tools", []))''',
         '''        offered = [t.get("name") for t in req_body.get("tools", [])]
        for it in inp:
            if isinstance(it, dict) and it.get("type") == "additional_tools":
                offered += [t.get("name") for ns in it.get("tools", []) for t in ns.get("tools", [])]
        side = not any(n in ("exec_command", "shell", "exec") for n in offered)''')
    edit("engine.py", '''                 conversation=cv["id"], turn=cv["turns"], stop=stop, joined=joined,''',
         '''                 conversation=cv["id"], turn=cv["turns"], stop=stop, joined=joined, session=session,''')
if one_main:
    edit("night.py", 'json.dump(scripts(main_home, fork_home)', 'json.dump(scripts(main_home, main_home)')
    s = open(os.path.join(out, "night.py")).read()
    a = s.index('    time.sleep(6)\n    runs.append(launch("main", main_home, app, claude_argv(ASK_4)')
    b = s.index('    if REAL:\n        time.sleep(5)')
    open(os.path.join(out, "night.py"), "w").write(s[:a] + '''    time.sleep(2)
    write(f"{main_home}/.codex/config.toml", f'model = "{CODEX_MODEL}"\\nmodel_provider = "boundary"\\n[model_providers.boundary]\\nname = "boundary"\\nbase_url = "http://127.0.0.1:{PORTS["main"]["openai"]}/v1"\\nwire_api = "responses"\\nenv_key = "OPENAI_KEY_PLACEHOLDER"\\n')
    cx = launch("main", main_home, app, [CODEX, "exec", "--json", "--dangerously-bypass-approvals-and-sandbox", "--skip-git-repo-check", ASK_3], "main-launch-4",
                {"CODEX_HOME": f"{main_home}/.codex", "OPENAI_KEY_PLACEHOLDER": "placeholder-not-a-key", "RUST_LOG": "warn"}, PORTS["main"])
    time.sleep(4)
    runs.append(launch("main", main_home, app, claude_argv(ASK_4), "main-launch-3", claude_env(PORTS["main"]), PORTS["main"]))
    runs.append(cx)
''' + s[b:])
print(out)

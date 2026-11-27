"""A scripted program for the cases no agent program installed here produces on demand: Chat
Completions with and without `include_usage`, Gemini's three ways of answering, calls dropped in
the middle, and a model host Silo cannot count. Runs on its own computer (127.0.0.7), through the
boundary, with Silo's authority in SSL_CERT_FILE. Prints one line per call."""
import http.client, json, os, ssl, sys, time

CTX = ssl.create_default_context(cafile=os.environ.get("SSL_CERT_FILE"))
OPENAI = "sk-proj-script-fake"; ANTH = "sk-ant-api03-script-fake"; GOOG = "AIzaScriptFake000"

def post(host, path, body, headers, drop_after=None):
    c = http.client.HTTPSConnection(host, 443, context=CTX, timeout=120)
    b = json.dumps(body).encode()
    c.request("POST", path, b, dict({"content-type": "application/json"}, **headers))
    r = c.getresponse(); got = b""
    while True:
        chunk = r.read1(4096) if hasattr(r, "read1") else r.read(4096)
        if not chunk: break
        got += chunk
        if drop_after and len(got) >= drop_after:
            r.fp.raw._sock.close() if c.sock is None else c.sock.close()
            return r.status, got, "dropped"
    return r.status, got, "whole"

def say(*a):
    print(*a, flush=True)

history = [{"role": "system", "content": "You are terse."}, {"role": "user", "content": "Say something. " * 400}]
for i in range(3):
    s, b, how = post("api.openai.com", "/v1/chat/completions", {"model": "gpt-6.2-sol", "stream": True, "messages": history}, {"authorization": "Bearer " + OPENAI})
    say("chat stream, no usage", s, how, len(b)); history.append({"role": "assistant", "content": "ok " * 50}); history.append({"role": "user", "content": "more " * 300})
for i in range(3):
    s, b, how = post("api.openai.com", "/v1/chat/completions", {"model": "gpt-6.2-sol", "stream": True, "stream_options": {"include_usage": True}, "messages": history}, {"authorization": "Bearer " + OPENAI})
    say("chat stream, include_usage", s, how, len(b)); history.append({"role": "assistant", "content": "ok " * 50}); history.append({"role": "user", "content": "more " * 300})
s, b, how = post("api.openai.com", "/v1/chat/completions", {"model": "gpt-6.2-sol", "messages": history}, {"authorization": "Bearer " + OPENAI})
say("chat, not streamed", s, how, len(b))

contents = [{"role": "user", "parts": [{"text": "Explain. " * 600}]}]
tools = [{"functionDeclarations": [{"name": "run_shell_command", "description": "run", "parameters": {"type": "object", "properties": {"command": {"type": "string"}}}}]}]
for path, label in [(":streamGenerateContent?alt=sse", "gemini sse"), (":streamGenerateContent", "gemini json array"), (":generateContent", "gemini not streamed")]:
    for i in range(2):
        s, b, how = post("generativelanguage.googleapis.com", "/v1beta/models/gemini-3.1-pro-preview" + path, {"contents": contents, "tools": tools}, {"x-goog-api-key": GOOG})
        say(label, s, how, len(b))
        contents = contents + [{"role": "model", "parts": [{"text": "step"}]}, {"role": "user", "parts": [{"text": "go on " * 200}]}]

# Calls the program drops: while the model thinks (nothing visible yet), and while text streams.
msgs = [{"role": "user", "content": "SUBAGENT 9: think hard. " * 300}]
tools_a = [{"name": "Bash", "description": "run", "input_schema": {"type": "object"}}]
for drop, label in [(600, "anthropic dropped while thinking"), (600, "anthropic dropped while thinking"), (3000, "anthropic dropped in text"), (None, "anthropic whole")]:
    s, b, how = post("api.anthropic.com", "/v1/messages", {"model": "claude-opus-5-5", "max_tokens": 32000, "stream": True, "messages": msgs, "tools": tools_a}, {"x-api-key": ANTH, "anthropic-version": "2023-06-01"}, drop_after=drop)
    say(label, s, how, len(b)); time.sleep(1.5)
inp = [{"type": "message", "role": "user", "content": "Plan the work. " * 500}]
for drop, label in [(400, "responses dropped while reasoning"), (None, "responses whole")]:
    s, b, how = post("api.openai.com", "/v1/responses", {"model": "gpt-6.2-sol", "stream": True, "input": inp, "tools": [{"type": "function", "name": "shell", "parameters": {}}]}, {"authorization": "Bearer " + OPENAI}, drop_after=drop)
    say(label, s, how, len(b)); time.sleep(1.5)

# A model host Silo cannot count.
try:
    s, b, how = post("api.mistral.ai", "/v1/chat/completions", {"model": "mistral-large", "messages": history[:2]}, {"authorization": "Bearer mistral-fake"})
    say("uncountable host", s, b[:200])
except Exception as e:
    say("uncountable host", "error", e)
time.sleep(2)

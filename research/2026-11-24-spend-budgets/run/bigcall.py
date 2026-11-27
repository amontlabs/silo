"""One conversation that sends small calls, then one call with a very large input, under a
budget of $1 on its computer: whether the large call leaves when little of the budget is left."""
import http.client, json, os, ssl, time
CTX = ssl.create_default_context(cafile=os.environ.get("SSL_CERT_FILE"))
def post(body):
    c = http.client.HTTPSConnection("api.anthropic.com", 443, context=CTX, timeout=300)
    c.request("POST", "/v1/messages", json.dumps(body).encode(), {"content-type": "application/json", "x-api-key": "sk-ant-api03-script-fake", "anthropic-version": "2023-06-01"})
    r = c.getresponse(); b = r.read(); return r.status, b
tools = [{"name": "Bash", "description": "run", "input_schema": {"type": "object"}}]
msgs = [{"role": "user", "content": "Work. " * 2000}]
for i in range(40):
    big = i == int(os.environ.get("BIG_AT", "9"))
    if big:
        # A tool result of 2.4 MB: about 800,000 tokens of new input, written to the cache.
        msgs.append({"role": "assistant", "content": [{"type": "tool_use", "id": f"t{i}", "name": "Bash", "input": {"command": "cat dump.sql"}}]})
        msgs.append({"role": "user", "content": [{"type": "tool_result", "tool_use_id": f"t{i}", "content": "INSERT INTO t VALUES (1);\n" * 92000, "cache_control": {"type": "ephemeral", "ttl": "1h"}}]})
    s, b = post({"model": "claude-opus-5-5", "max_tokens": 8000, "stream": True, "messages": msgs, "tools": tools})
    print(i, "big" if big else "small", s, b[:160] if s != 200 else "", flush=True)
    if s != 200: break
    msgs.append({"role": "assistant", "content": [{"type": "text", "text": "ok"}]}); msgs.append({"role": "user", "content": "go on " * 100})

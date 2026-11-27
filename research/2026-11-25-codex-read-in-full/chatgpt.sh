#!/bin/bash
# Writes a ChatGPT sign-in as the auth.json Codex keeps in $CODEX_HOME, then runs the command.
# Made-up tokens: the id_token and access_token are unsigned JWTs with the claims Codex reads.
# EXPIRED=1 writes an access token that expired an hour ago and a last refresh 30 days old, so
# Codex refreshes it before its first call.
python3 - "$CODEX_HOME/auth.json" <<'PY'
import base64, json, os, sys, time, datetime
b = lambda o: base64.urlsafe_b64encode(json.dumps(o).encode()).rstrip(b"=").decode()
now = int(time.time()); old = os.environ.get("EXPIRED") == "1"
claims = {"email": "night@example.org", "exp": now - 3600 if old else now + 86400, "iat": now - 7200,
          "https://api.openai.com/auth": {"chatgpt_plan_type": "pro", "chatgpt_account_id": "acct-test", "chatgpt_user_id": "user-test"}}
jwt = b({"alg": "none", "typ": "JWT"}) + "." + b(claims) + ".sig"
last = datetime.datetime.now(datetime.timezone.utc) - datetime.timedelta(days=30 if old else 0)
json.dump({"OPENAI_API_KEY": None, "tokens": {"id_token": jwt, "access_token": jwt, "refresh_token": "rt-test", "account_id": "acct-test"},
           "last_refresh": last.isoformat().replace("+00:00", "Z")}, open(sys.argv[1], "w"))
PY
exec "$@"

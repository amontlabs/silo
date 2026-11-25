#!/bin/bash
# Every mode, one after the other. Output: results.txt.
H=$(cd $(dirname $0); pwd); O=${1:-$H/out}; mkdir -p $O; : > $O/results.txt
declare -A URL=([plain]=http://localhost:7000/ [csp]=http://localhost:7000/ [origin]=http://3000.c1.silo.localhost:7000/ [shell]=http://preview.silo.localhost:7000/c1/3000 [shell-webrtc]=http://preview.silo.localhost:7000/c1/3000 [browser]=http://3000.c1.silo.localhost:7000/ [shell-browser]=http://preview.silo.localhost:7000/c1/3000)
for m in plain csp origin shell shell-webrtc browser shell-browser; do
  rm -f $O/hits.jsonl; MD=${m%-webrtc}; [ $m = shell-browser ] && MD=shell; PROXY=$(case $m in *browser) echo 1;; esac) MODE=$MD CSP_WEBRTC=$([ $m = shell-webrtc ] && echo 1) python3 $H/servers.py $O/hits.jsonl & S=$!; sleep 0.8
  ARGS='[]'; case $m in *browser) ARGS='["--proxy-server=http://127.0.0.1:7100","--proxy-bypass-list=<-loopback>","--force-webrtc-ip-handling-policy=disable_non_proxied_udp"]';; esac
  B=$(cd /opt/node-tools && ARGS="$ARGS" node $H/browse.js ${URL[$m]})
  sleep 1; kill $S; wait $S 2>/dev/null
  python3 - $m "$B" $O/hits.jsonl >> $O/results.txt <<'P'
import json, sys
m, b, f = sys.argv[1], json.loads(sys.argv[2]), sys.argv[3]
hits = [json.loads(l) for l in open(f)] if __import__("os").path.exists(f) else []
out = sorted({h["via"] + (" +secret" if h.get("secret") else "") + (" +mac-data" if h.get("mac") else "") + (" +cookie:" + h["cookie"] if h.get("cookie") else "") for h in hits if h["to"] == "attacker"})
mac = sorted({h["origin"] for h in hits if h["to"] == "mac-service"})
print(f"== {m}\n   reached the attacker ({len(out)}): " + ", ".join(out))
print(f"   reached the Mac service from: {mac or 'nothing'}")
ok = sorted({h["what"] for h in hits if h["to"] == "computer"})
print(f"   what the app did on its own origin: {', '.join(ok)}")
den = [h for h in hits if h["to"].startswith("denied")]
if den: print(f"   denied and recorded by the computer's rules: {len(den)} requests, {sum(h['secret'] for h in den)} of them carrying the secret")
print(f"   downloads: {b['downloads']}, new windows: {b['popups']}, page title: {b.get('title')!r}")
left = {k: v for k, v in b['final'].items() if '4000' in v}
print(f"   the page ended on the attacker's site after: {', '.join(left) or 'nothing'}")
P
done
cat $O/results.txt

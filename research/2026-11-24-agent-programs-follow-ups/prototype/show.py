import json, sys
for l in open(sys.argv[1] + "/requests.jsonl"):
    d = json.loads(l)
    if d.get("event") == "dns" and d.get("type") != 1: continue
    print({k: d.get(k) for k in ('t','event','name','sni','alpn_offered','alpn','method','host','path','answer','decided','error','client_gave_up_after','content_encoding') if d.get(k) is not None})

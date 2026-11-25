#!/bin/bash
# Asks the real provider hosts, with no credential, which spellings of a model route they answer
# as that route (an authentication error) and which they do not (404, redirect), and whether they
# read a header placed after a bare LF or on a folded line. One request each, no key, no body.
# The answers came through this container's HTTPS proxy, which ends TLS itself (see README).
H=$(cd $(dirname $0); pwd)
probe() { host=$1; shift; for t in "$@"; do printf '%-40s %-58s %s\n' "$host" "$t" "$(curl -s -m 15 -o /tmp/live.b -w '%{http_code}' --path-as-is --request-target "$t" -X POST https://$host/ -H 'content-type: application/json' -d '{}') $(head -c 70 /tmp/live.b | tr -d '\n')"; done; }
probe api.anthropic.com /v1/messages /v1/nonexistent https://api.anthropic.com/v1/messages //v1/messages /v1//messages /v1/./messages /v1/x/../messages /%761/messages /v1/messages/ /V1/MESSAGES "/v1/messages;x" /v1/messages%2f
probe api.openai.com /v1/responses /v1/nonexistent //v1/responses /v1/./responses /v1/responses/ /v1/chat/completions/
probe generativelanguage.googleapis.com /v1beta/models/gemini-2.5-flash:generateContent /v1beta/models/gemini-2.5-flash:nonexistent /v1beta/models/gemini-2.5-flash:generateContent/ /v1beta/models/gemini-2.5-flash%3AgenerateContent
echo; echo "Does the proxy pass a path as written? A Go server redirects a path with a dot segment:"
echo "httpbingo.org /./anything -> $(curl -s -m 15 -o /dev/null -w '%{http_code}' --path-as-is --request-target /./anything https://httpbingo.org/anything)"
echo; echo "Headers after a bare LF or on a folded line (a key message means the host read the header):"
for host in api.anthropic.com api.openai.com; do
  [ $host = api.anthropic.com ] && k='x-api-key: bogus' p=/v1/messages || { k='authorization: Bearer bogus'; p=/v1/responses; }
  for f in plain lf fold; do case $f in plain) h="$k\\r\\n";; lf) h="x-pad: a\\n$k\\r\\n";; fold) h="x-pad: a\\r\\n $k\\r\\n";; esac
    echo "$host $f: $(python3 $H/live.py $host "POST $p HTTP/1.1\\r\\nhost: $host\\r\\n${h}content-type: application/json\\r\\ncontent-length: 2\\r\\nconnection: close\\r\\n\\r\\n{}" | cut -c1-150)"; done; done

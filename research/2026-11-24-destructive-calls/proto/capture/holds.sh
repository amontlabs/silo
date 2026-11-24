#!/usr/bin/env bash
# holds.sh <cli>: replays one destructive command of that CLI against a 2 s hold, once per
# refusal style, and saves what the CLI printed (outputs/<cli>-hold-<style>.txt).
# The canned answers in canned/<cli>.json let the CLI reach the destructive request.
# Run from a directory the CLI can use (vercel needs a linked project: /opt/clis/vproj).
H="$(cd "$(dirname "$0")" && pwd)"
S="Held by Silo and not answered: deleting volume X needs your yes."
cli=$1
case $cli in
  gh)       re='DELETE'; env=(GH_TOKEN=test-token-123); cmd=(gh repo delete o/r --yes)
            native='{"message":"SENTENCE","documentation_url":"https://docs.github.com/rest"}' ;;
  railway)  re='volumeDelete'; env=(RAILWAY_TOKEN=test-token-123); cmd=(railway volume delete --volume vol-1 --yes)
            native='{"data":null,"errors":[{"message":"SENTENCE","extensions":{"code":"FORBIDDEN"}}]}' ;;
  vercel)   re='DELETE'; env=(); cmd=(vercel project rm my-app --confirm-name my-app --scope acme --token testtoken123)
            native='{"error":{"code":"forbidden","message":"SENTENCE"}}' ;;
  fly)      re='DELETE'; env=(FLY_API_TOKEN=test-token-123); cmd=(flyctl volumes destroy vol_123 -a my-app --yes)
            native='{"error":"SENTENCE"}' ;;
  wrangler) re='DELETE'; env=(CLOUDFLARE_API_TOKEN=test-token-123 CLOUDFLARE_ACCOUNT_ID=acc123 WRANGLER_SEND_METRICS=false); cmd=(wrangler d1 delete mydb -y)
            native='{"success":false,"errors":[{"code":10000,"message":"SENTENCE"}],"messages":[],"result":null}' ;;
  stripe)   re='DELETE'; env=(STRIPE_API_KEY=sk_test_fake123); cmd=(stripe customers delete cus_123 --confirm)
            native='{"error":{"type":"invalid_request_error","code":"held_by_silo","message":"SENTENCE"}}' ;;
  supabase) re='DELETE'; env=(SUPABASE_ACCESS_TOKEN=sbp_FAKE); cmd=(supabase projects delete abcdefghijklmnopqrst --yes)
            native='{"message":"SENTENCE"}' ;;
  neon)     re='DELETE'; env=(NEON_API_KEY=test-token-123); cmd=(neonctl projects delete proj-123 --output json)
            native='{"code":"","message":"SENTENCE"}' ;;
  heroku)   re='DELETE'; env=(HEROKU_API_KEY=test-token-123); cmd=(heroku apps:destroy -a my-app --confirm my-app)
            native='{"id":"forbidden","message":"SENTENCE"}' ;;
  *) echo "unknown cli"; exit 1 ;;
esac
styles=(json403 text403 native)
[ "$cli" = railway ] && styles+=(gql200)
for st in "${styles[@]}"; do
  if [ "$st" = native ]; then
    args=(--hold-style custom --hold-status 403 --hold-body "$native")
  else
    args=(--hold-style "$st")
  fi
  "$H/srv" "$cli" --hold "$re" --hold-secs 2 "${args[@]}" >/dev/null
  n0=$(wc -l < "$H/captures/$cli.jsonl")
  "$H/timed" "$cli-hold-$st" "${env[@]}" -- "${cmd[@]}" >/dev/null
  sleep 1
  held=$(tail -n +"$((n0 + 1))" "$H/captures/$cli.jsonl" | grep -c '"event": "hold_start"')
  echo "--- matching requests held: $held" >> "$H/outputs/$cli-hold-$st.txt"
done

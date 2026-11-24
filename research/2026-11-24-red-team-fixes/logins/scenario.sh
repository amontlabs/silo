#!/bin/bash
# Every case, for one agent program, through the vault or through a plain boundary (`pass`: it reads
# the traffic but keeps no login, which is the plan's answer in `logins` today).
# usage: scenario.sh <cc|cx> <out> <vault|pass>
H=$(cd $(dirname $0); pwd); P=$1; O=$2; M=$3; rm -rf $O; mkdir -p $O/h1 $O/h2; chown cuser $O/h1 $O/h2
pkill -f "logins/(vault|providers).py"; sleep 0.3
ACCESS_TTL=${ACCESS_TTL:-3600} $H/up.sh $O $M
if [ $P = cc ]; then CRED=.claude/.credentials.json; else CRED=.codex/auth.json; fi
setup() {  # what the program keeps beside its login, copied to a new home
  if [ $P = cc ]; then cp $O/h1/.claude.json $1/ 2>/dev/null; else mkdir -p $1/.codex; cp $H/codex-config.toml $1/.codex/config.toml; fi
  chown -R cuser $1; }
signin() { $H/signin_$P.sh c1 $O/h1 $O/ca.crt; [ $P = cx ] && setup $O/h1; }
run() { $H/$P.sh $1 $2 $O/ca.crt; }
say() { echo; echo "== $*"; }
say "1. sign in inside computer c1, the vendor's own way"; signin
say "2. work in c1"; run c1 $O/h1
say "3. hostile code copies c1's login file out, and uses it from the internet"
cp $O/h1/$CRED $O/stolen.json; python3 $H/attacker.py $P $O/stolen.json ${CERTS:-/tmp/claude-0/w/certs}/up-ca.crt
say "4. time passes (every access token expires), and c1 works"; touch $O/expire; sleep 0.1; run c1 $O/h1
[ $M = pass ] && { say "   (the login is spent: sign in again in c1)"; signin; }
say "5. a fork c2 keeps the login (fork-and-sign-in: you said so)"
mkdir -p $O/h2/$(dirname $CRED)
if [ $M = vault ]; then
  curl -s -XPOST localhost:9900/grant -d '{"computer":"c2"}' | python3 -c "
import json,sys; g=json.load(sys.stdin); c=json.load(open('$O/h1/$CRED'))
if '$P'=='cc': c['claudeAiOauth'].update(accessToken=g['access'], refreshToken=g['refresh'], expiresAt=int(g['exp']*1000))
else: c['tokens'].update(access_token=g['access'], refresh_token=g['refresh'], id_token=g['id_token'])
json.dump(c, open('$O/h2/$CRED','w'))"
else
  cp $O/h1/$CRED $O/h2/$CRED
fi
setup $O/h2
say "6. both computers work, three rounds; every access token expires before each round, and c2 runs 11 s after c1"
for k in 1 2 3; do touch $O/expire; sleep 0.1; echo -n "c1: "; run c1 $O/h1; sleep 11; echo -n "c2: "; run c2 $O/h2; done
if [ $M = vault ]; then
  say "7. hostile code copies c1's login file into c2 and uses it there"
  mkdir -p $O/h3/$(dirname $CRED); cp $O/h1/$CRED $O/h3/$CRED; setup $O/h3
  echo -n "c2 with c1's file: "; run c2 $O/h3
fi
say "what the vault did"; curl -s localhost:9900/stats; echo
kill $(cat $O/pp $O/vp)
python3 $H/show.py $O > $O/show.txt
python3 - $O <<'P'
import sys,glob,os
o=sys.argv[1]; issued=[l for l in open(f"{o}/issued.txt").read().split() if l]
files=[os.path.join(d,f) for h in glob.glob(f"{o}/h*") for d,_,fs in os.walk(h) for f in fs]
found={t for f in files for t in issued if t in open(f,errors="replace").read()}
stolen=open(f"{o}/stolen.json").read()
print(f"tokens the provider issued: {len(issued)}; real ones in the file the attacker copied at step 3: {sum(t in stolen for t in issued)}; in the computers' files at the end: {len(found)} ({len(files)} files searched)")
P

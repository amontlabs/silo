#!/usr/bin/env bash
# collect.sh - guest-side state manifest, format 2 (docs/v1/PHASE2_CONTRACT.md).
# Run inside a guest:  bash collect.sh > manifest.tsv
# Prints one tab-separated record per line. Read-only. Works with bash 3.2 (macOS) and 5.x.
# Everything lives inside main() so that bash -s has parsed the whole script
# before any child process can touch stdin.
main() {
exec </dev/null
export LC_ALL=C
cd / 2>/dev/null
export HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_ANALYTICS=1 HOMEBREW_NO_ENV_HINTS=1 \
  HOMEBREW_NO_INSTALL_CLEANUP=1 NO_COLOR=1 GIT_TERMINAL_PROMPT=0 GIT_OPTIONAL_LOCKS=0
START=$(date +%s)
OS=$(uname -s)
[ -n "${HOME:-}" ] || HOME=$(cd ~ 2>/dev/null && pwd)
HOME=${HOME%/}
export PATH="$PATH:/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:$HOME/.cargo/bin:$HOME/.local/bin"
TMP=$(mktemp -d 2>/dev/null || mktemp -d -t manifest) || return 1
trap 'rm -rf "$TMP"' EXIT
bs='\'; tb=$'\t'; nl=$'\n'

have() { command -v "$1" >/dev/null 2>&1; }
esc() { local s=$1; s=${s//"$bs"/"$bs$bs"}; s=${s//"$tb"/"${bs}t"}; s=${s//"$nl"/"${bs}n"}; printf '%s' "$s"; }
if have sha256sum; then SHA=(sha256sum); elif have shasum; then SHA=(shasum -a 256); else SHA=(cat); fi

# PROC first, so the helpers started below do not show up.
PROCS=$(ps -A -o pid=,ppid=,comm= 2>/dev/null | awk -v me=$$ '
  $2==2 || $1==2 || $1==0 || $1==me || $2==me {next}
  { sub(/^ *[0-9]+ +[0-9]+ +/,""); sub(/.*\//,""); sub(/ +$/,"");
    if ($0=="" || $0 ~ /^(ps|bash|sh|awk|sort|sed|tr|uniq|cat|msb-guest|-bash)$/) next; print }' | sort -u)

printf 'META\tformat\t2\n'
printf 'META\tos\t%s\n' "$OS"
printf 'META\thome\t%s\n' "$(esc "$HOME")"
printf 'META\tuser\t%s\n' "$(id -un 2>/dev/null)"

# ---------------------------------------------------------------- files
ROOTS=()
add_root() { [ -d "$1" ] && ROOTS[${#ROOTS[@]}]=$1; }
if [ "$OS" = Darwin ]; then
  add_root "$HOME"; add_root /usr/local; add_root /opt/homebrew/etc
  add_root /Library/LaunchAgents; add_root /Library/LaunchDaemons
else
  for r in /etc /root /home /usr/local /opt /srv /var/spool/cron; do add_root "$r"; done
  case "$HOME" in /root|/root/*|/home|/home/*) ;; *) add_root "$HOME";; esac
fi

PR=( -type d '(' -false )
for n in .git node_modules .cache .npm .rustup target __pycache__ .venv venv .Trash Caches GPUCache CachedData 'Code Cache'; do
  PR[${#PR[@]}]=-o; PR[${#PR[@]}]=-name; PR[${#PR[@]}]=$n
done
for pth in '*/.cargo/registry' /opt/homebrew/Cellar /usr/local/Cellar /usr/local/lib/node_modules; do
  PR[${#PR[@]}]=-o; PR[${#PR[@]}]=-path; PR[${#PR[@]}]=$pth
done
if [ "$OS" = Darwin ]; then
  # prune every ~/Library child except the allow-listed ones
  PR=( "${PR[@]}" -o '(' -path "$HOME/Library/*" ! -path "$HOME/Library/*/*" \
       ! -name LaunchAgents ! -name Preferences ! -name 'Application Support' ')' )
fi
PR[${#PR[@]}]=')'

XSH='for f do printf "X\t%s\n" "$(printf %s "$f" | tr "\n\t" "\001\002")"; done'
RAW=$TMP/raw
if [ ${#ROOTS[@]} -gt 0 ]; then
  if find /dev/null -printf '' >/dev/null 2>&1; then
    FMT='%y\t%s\t%T@\t%p\t%l\n'
    find -H "${ROOTS[@]}" '(' "${PR[@]}" ')' -prune -printf "$FMT" -o \
      '(' -type f -o -type l ')' '(' -path '*[[:cntrl:]]*' -exec sh -c "$XSH" sh '{}' + -o -printf "$FMT" ')' \
      >"$RAW" 2>/dev/null
  else
    SF='%HT%t%z%t%m%t%N%t%Y'
    find -H "${ROOTS[@]}" '(' "${PR[@]}" ')' -prune -exec stat -f "$SF" '{}' + -o \
      '(' -type f -o -type l ')' '(' -path '*[[:cntrl:]]*' -exec sh -c "$XSH" sh '{}' + -o -exec stat -f "$SF" '{}' + ')' \
      >"$RAW" 2>/dev/null
  fi
fi
: >>"$RAW"

# files <= 1 MiB get hashed
awk -F'\t' '($1=="f"||$1=="Regular File") && NF>=5 && $2+0<=1048576 {print $4}' "$RAW" >"$TMP/tohash"
tr '\n' '\0' <"$TMP/tohash" | xargs -0 -n 500 "${SHA[@]}" >"$TMP/hashes" 2>/dev/null
: >>"$TMP/hashes"

awk -F'\t' -v H="$TMP/hashes" -v PF="$TMP/pdirs" '
function esc(s,  o,i,c){ if (index(s,"\\")==0) return s; o=""
  for(i=1;i<=length(s);i++){c=substr(s,i,1); o=o (c=="\\"?"\\\\":c)} return o }
function xesc(s,  o,i,c){ o=""
  for(i=1;i<=length(s);i++){c=substr(s,i,1)
    o=o (c=="\001"?"\\n":(c=="\002"?"\\t":(c=="\\"?"\\\\":c)))} return o }
FILENAME==H { if (substr($0,1,1)=="\\" || length($0)<67) next; hs[substr($0,67)]=substr($0,1,64); next }
$1=="X" { print "F\t" xesc($2) "\t-\t-\t-"; next }
NF>=5 {
  t=$1; m=$3; sub(/\..*/,"",m)
  if (t=="d"||t=="Directory") print $4 >PF
  else if (t=="f"||t=="Regular File") print "F\t" esc($4) "\t" $2 "\t" m "\t" (($4 in hs)?hs[$4]:"-")
  else if (t=="l"||t=="Symbolic Link") print "L\t" esc($4) "\t" $2 "\t" m "\t" esc($5)
}' "$TMP/hashes" "$RAW" >"$TMP/fs"
: >>"$TMP/pdirs"

sort "$TMP/pdirs" | {
  np=0
  while IFS= read -r d; do
    n=-
    if [ $np -lt 400 ]; then
      n=$(find "$d" -maxdepth 2 2>/dev/null | wc -l | tr -d ' ')
      np=$((np+1))
    fi
    printf 'P\t%s\t%s\n' "$(esc "$d")" "$n"
  done
} >>"$TMP/fs"
sort -t "$tb" -k2,2 "$TMP/fs"

# ---------------------------------------------------------------- packages
if have dpkg-query; then
  dpkg-query -W -f='${db:Status-Abbrev}\t${binary:Package}\t${Version}\n' 2>/dev/null |
    awk -F'\t' '$1 ~ /^ii/ {printf "PKG\tdpkg\t%s\t%s\n",$2,$3}'
  have apt-mark && apt-mark showmanual 2>/dev/null | awk 'NF{printf "PKGM\tdpkg\t%s\n",$1}'
  # PKGD mgr name dep,dep: direct requirements (first alternative, no versions).
  dpkg-query -W -f='${binary:Package}\t${Depends}\n' 2>/dev/null | awk -F'\t' '
    NF>=2 && $2!="" { n=split($2,a,", "); out=""
      for (i=1;i<=n;i++) { d=a[i]; sub(/ *\|.*/,"",d); sub(/ *\(.*/,"",d); sub(/:[a-z0-9]+$/,"",d)
        if (d!="") out=out (out==""?"":",") d }
      if (out!="") printf "PKGD\tdpkg\t%s\t%s\n",$1,out }'
fi
if have brew; then
  brew list --versions 2>/dev/null | awk 'NF>=2{v=$2; for(i=3;i<=NF;i++) v=v","$i; printf "PKG\tbrew\t%s\t%s\n",$1,v}'
  brew list --cask --versions 2>/dev/null | awk 'NF>=2{v=$2; for(i=3;i<=NF;i++) v=v","$i; printf "PKG\tbrew-cask\t%s\t%s\n",$1,v}'
  brew leaves 2>/dev/null | awk 'NF{printf "PKGM\tbrew\t%s\n",$1}'
  brew deps --installed --direct 2>/dev/null | awk -F': ' 'NF>=2 && $2!="" {gsub(/ +/,",",$2); printf "PKGD\tbrew\t%s\t%s\n",$1,$2}'
fi
if have npm; then
  npm ls -g --depth=0 --json 2>/dev/null | awk '
    /^    "[^"]+": \{/ { split($0,a,"\""); name=a[2]; next }
    /^      "version": "/ { split($0,a,"\""); if (name!="") printf "PKG\tnpm\t%s\t%s\n",name,a[4]; name="" }'
fi
for py in python3 python; do
  if have $py; then
    $py -m pip list --format=freeze 2>/dev/null | awk -F'==' 'NF==2{printf "PKG\tpip\t%s\t%s\n",$1,$2}'
    $py -m pip list --format=freeze 2>/dev/null | awk -F'==' 'NF==2{print $1}' | xargs $py -m pip show 2>/dev/null |
      awk '/^Name: /{n=$2} /^Requires: /{r=$0; sub(/^Requires: */,"",r); gsub(/, */,",",r); if (n!="" && r!="") printf "PKGD\tpip\t%s\t%s\n",n,r; n=""}'
    break
  fi
done
have pipx && pipx list --short 2>/dev/null | awk 'NF>=2{printf "PKG\tpipx\t%s\t%s\n",$1,$2}'
have cargo && cargo install --list 2>/dev/null | awk '/^[^ ]/ {v=$2; sub(/^v/,"",v); sub(/:$/,"",v); printf "PKG\tcargo\t%s\t%s\n",$1,v}'
if have gem; then
  gem list --local 2>/dev/null | awk 'NF>=2 && $2 ~ /^\(/ {v=$0; sub(/^[^(]*\(/,"",v); sub(/\)$/,"",v); gsub(/, /,",",v); printf "PKG\tgem\t%s\t%s\n",$1,v}'
fi
have snap && snap list 2>/dev/null | awk 'NR>1 && NF>=2{printf "PKG\tsnap\t%s\t%s\n",$1,$2}'

# ---------------------------------------------------------------- git
# GIG repo path size mtime sha256|-: gitignored files (outside build and cache trees), so a
# changed .env shows. Files up to 1 MiB are hashed; at most 2000 files per repo.
if stat --version >/dev/null 2>&1; then
  gstat() { xargs -0 stat --printf='%s\t%Y\t%n\n'; }
else
  gstat() { xargs -0 stat -f '%z%t%m%t%N'; }
fi
gig_repo() {
  local R=$1 er=$2
  case "$R" in /opt/homebrew|/usr/local/Homebrew) return 0;; esac
  gg ls-files --others --ignored --exclude-standard 2>/dev/null | grep -v '[[:cntrl:]]' |
    awk 'BEGIN{n=split("node_modules .cache .npm .rustup .cargo target __pycache__ .venv venv .Trash dist build .next .nuxt .turbo .parcel-cache coverage .gradle .tox .mypy_cache .pytest_cache .ruff_cache .terraform .svelte-kit",a," "); for(i=1;i<=n;i++)P[a[i]]=1}
         {m=split($0,c,"/"); for(i=1;i<=m;i++) if (c[i] in P) next; print}' | head -n 2000 >"$TMP/gl"
  [ -s "$TMP/gl" ] || return 0
  awk -v r="$R" '{print r "/" $0}' "$TMP/gl" | tr '\n' '\0' | gstat >"$TMP/gs" 2>/dev/null
  : >>"$TMP/gs"
  awk -F'\t' '$1+0<=1048576 {print $3}' "$TMP/gs" | tr '\n' '\0' | xargs -0 -n 500 "${SHA[@]}" >"$TMP/gh" 2>/dev/null
  : >>"$TMP/gh"
  awk -F'\t' -v H="$TMP/gh" -v r="$er" -v pre="$R/" '
    function esc(s,  o,i,c){ if (index(s,"\\")==0) return s; o=""
      for(i=1;i<=length(s);i++){c=substr(s,i,1); o=o (c=="\\"?"\\\\":c)} return o }
    FILENAME==H { if (substr($0,1,1)=="\\" || length($0)<67) next; hs[substr($0,67)]=substr($0,1,64); next }
    NF>=3 { p=$3; printf "GIG\t%s\t%s\t%s\t%s\t%s\n", r, esc(substr(p,length(pre)+1)), $1, $2, ((p in hs)?hs[p]:"-") }
  ' "$TMP/gh" "$TMP/gs"
}

if have git; then
  GR=()
  for r in "$HOME" /srv /opt; do [ -d "$r" ] && GR[${#GR[@]}]=$r; done
  [ "$OS" = Linux ] && [ -d /home ] && GR[${#GR[@]}]=/home
  GP=( -type d '(' -false )
  for n in node_modules .cache .npm .rustup .cargo target __pycache__ .venv venv .Trash; do
    GP[${#GP[@]}]=-o; GP[${#GP[@]}]=-name; GP[${#GP[@]}]=$n
  done
  GP[${#GP[@]}]=-o; GP[${#GP[@]}]=-path; GP[${#GP[@]}]="$HOME/Library"
  GP[${#GP[@]}]=')'
  if [ ${#GR[@]} -gt 0 ]; then
    find -H "${GR[@]}" -maxdepth 4 '(' "${GP[@]}" ')' -prune -o -name .git -prune -print 2>/dev/null |
      sed 's,/\.git$,,' | sort -u | awk '
        { q=$0; skip=0
          while ((i=match(q,/\/[^\/]*$/))>0) { q=substr(q,1,i-1); if (q in kept) {skip=1;break} }
          if (!skip) { kept[$0]=1; print } }' >"$TMP/repos"
    while IFS= read -r R; do
      [ -n "$R" ] || continue
      gg() { git -c safe.directory='*' -c core.quotepath=off -C "$R" "$@" 2>/dev/null; }
      [ "$(gg rev-parse --is-inside-work-tree)" = true ] || continue
      head=$(gg rev-parse HEAD); [ -n "$head" ] || head=-
      br=$(gg symbolic-ref --short -q HEAD); [ -n "$br" ] || br=detached
      gg status --porcelain=v1 -uall >"$TMP/st"
      dirty=$(grep -vc '^??' "$TMP/st"); untr=$(grep -c '^??' "$TMP/st")
      er=$(esc "$R")
      printf 'GIT\t%s\t%s\t%s\t%s\t%s\n' "$er" "$head" "$(esc "$br")" "$dirty" "$untr"
      head -n 200 "$TMP/st" | awk -v r="$er" '{xy=substr($0,1,2); gsub(/ /,"_",xy); printf "GITF\t%s\t%s\t%s\n",r,xy,substr($0,4)}'
      gig_repo "$R" "$er"
    done <"$TMP/repos"
  fi
fi

# ---------------------------------------------------------------- cron / services / ports / procs
me=$(id -un 2>/dev/null)
cronhash() { "${SHA[@]}" | awk '{print $1}'; }
# CRONF user path: the file the user's crontab lives in, so the diff shows one change.
cronf() {
  local f
  for f in "/var/spool/cron/crontabs/$1" "/var/spool/cron/$1" "/var/at/tabs/$1"; do
    [ -f "$f" ] && { printf 'CRONF\t%s\t%s\n' "$(esc "$1")" "$(esc "$f")"; return 0; }
  done
  return 0
}
if have crontab; then
  h=-; out=$(crontab -l 2>/dev/null) && h=$(printf '%s\n' "$out" | cronhash)
  printf 'CRON\t%s\t%s\n' "$me" "$h"
  cronf "$me"
  if [ "$OS" = Linux ] && [ "$(id -u)" = 0 ]; then
    for f in /var/spool/cron/crontabs/* /var/spool/cron/*; do
      [ -f "$f" ] || continue
      u=${f##*/}; [ "$u" = "$me" ] && continue
      out=$(crontab -u "$u" -l 2>/dev/null) && printf 'CRON\t%s\t%s\n' "$u" "$(printf '%s\n' "$out" | cronhash)"
      printf 'CRONF\t%s\t%s\n' "$(esc "$u")" "$(esc "$f")"
    done
  fi
fi
if [ "$OS" = Darwin ]; then
  have launchctl && launchctl list 2>/dev/null |
    awk 'NR>1 && NF>=3 && $3 !~ /^(com\.apple\.|application\.|0x)/ {printf "SVC\t%s\tloaded\n",$3}'
else
  have systemctl && systemctl list-unit-files --type=service --no-legend --no-pager 2>/dev/null |
    awk 'NF>=2{printf "SVC\t%s\t%s\n",$1,$2}'
fi
if have ss; then
  ss -ltnpH 2>/dev/null | awk '{n=split($4,a,":"); proc="-";
    if (match($0,/\(\("[^"]*"/)) proc=substr($0,RSTART+3,RLENGTH-4);
    printf "LISTEN\ttcp\t%s\t%s\n",a[n],proc}' | sort -u
elif have lsof; then
  lsof -nP -iTCP -sTCP:LISTEN 2>/dev/null | awk 'NR>1 && NF>=2{n=split($(NF-1),a,":"); printf "LISTEN\ttcp\t%s\t%s\n",a[n],$1}' | sort -u
elif [ -r /proc/net/tcp ]; then
  # Minimal guests ship neither ss nor lsof: state 0A is LISTEN; the process is not resolved.
  cat /proc/net/tcp /proc/net/tcp6 2>/dev/null | while read -r _ local _ st _; do
    [ "$st" = 0A ] && printf 'LISTEN\ttcp\t%d\t-\n' "0x${local##*:}"
  done | sort -u
fi
# ---------------------------------------------------------------- databases
# DB postgres <db> schema <sha256>, DBT postgres <db> <table> <sha256|-> <rows>: only when
# psql answers as the postgres OS user. DB sqlite <path> <sha256>: file digests under the
# user's own roots.
pgrun() {
  if [ "$(id -un 2>/dev/null)" = postgres ]; then "$@"
  elif [ "$(id -u)" = 0 ] && have runuser; then runuser -u postgres -- "$@"
  elif have sudo; then sudo -n -u postgres "$@"
  else return 127; fi
}
if have psql && have pg_dump && PGCONNECT_TIMEOUT=5 pgrun psql -XAtqc 'select 1' postgres >/dev/null 2>&1; then
  export PGCONNECT_TIMEOUT=5
  pgrun psql -XAtqc "select datname from pg_database where not datistemplate and datallowconn order by 1" postgres 2>/dev/null |
  while IFS= read -r db; do
    [ -n "$db" ] || continue
    edb=$(esc "$db")
    if pgrun pg_dump --schema-only "$db" >"$TMP/pgd" 2>/dev/null; then
      printf 'DB\tpostgres\t%s\tschema\t%s\n' "$edb" "$("${SHA[@]}" <"$TMP/pgd" | awk '{print $1}')"
    fi
    pgrun psql -XAtqd "$db" -c "select format('%I.%I', schemaname, tablename) from pg_tables where schemaname not in ('pg_catalog','information_schema') order by 1" 2>/dev/null |
      grep -v "['[:cntrl:]]" | head -n 300 |
    while IFS= read -r t; do
      [ -n "$t" ] || continue
      size=$(pgrun psql -XAtqd "$db" -c "select pg_total_relation_size('$t')" 2>/dev/null)
      rows=$(pgrun psql -XAtqd "$db" -c "select count(*) from $t" 2>/dev/null)
      [ -n "$rows" ] || rows=-
      h=-
      if [ "${size:-0}" -le 1073741824 ] 2>/dev/null; then
        pgrun psql -XAtqd "$db" -c "COPY $t TO STDOUT" 2>/dev/null | "${SHA[@]}" | awk '{print $1}' >"$TMP/pgh"
        [ "${PIPESTATUS[0]}" = 0 ] && h=$(cat "$TMP/pgh")
      fi
      printf 'DBT\tpostgres\t%s\t%s\t%s\t%s\n' "$edb" "$(esc "$t")" "${h:--}" "$rows"
    done
  done
fi
SQR=()
for r in "${ROOTS[@]}"; do
  case "$r" in /etc|/var/spool/cron|/Library/*|/opt/homebrew/etc) ;; *) SQR[${#SQR[@]}]=$r;; esac
done
if [ ${#SQR[@]} -gt 0 ]; then
  find -H "${SQR[@]}" '(' "${PR[@]}" ')' -prune -o -type f \
    '(' -name '*.sqlite' -o -name '*.sqlite3' -o -name '*.db' ')' -size -262144k -print 2>/dev/null |
    grep -v '[[:cntrl:]]' | sort -u >"$TMP/sqlite"
  if [ -s "$TMP/sqlite" ]; then
    tr '\n' '\0' <"$TMP/sqlite" | xargs -0 -n 50 "${SHA[@]}" 2>/dev/null | awk '
      function esc(s,  o,i,c){ if (index(s,"\\")==0) return s; o=""
        for(i=1;i<=length(s);i++){c=substr(s,i,1); o=o (c=="\\"?"\\\\":c)} return o }
      length($0)>=67 && substr($0,1,1)!="\\" { printf "DB\tsqlite\t%s\t%s\n", esc(substr($0,67)), substr($0,1,64) }'
  fi
fi

# ---------------------------------------------------------------- launch agents
# LAGENT scope label sha256 path (macOS LaunchAgents and LaunchDaemons).
if [ "$OS" = Darwin ]; then
  for pair in "user:$HOME/Library/LaunchAgents" "system:/Library/LaunchAgents" "system:/Library/LaunchDaemons"; do
    scope=${pair%%:*}; dir=${pair#*:}
    for f in "$dir"/*.plist; do
      [ -f "$f" ] || continue
      label=$(/usr/libexec/PlistBuddy -c 'Print :Label' "$f" 2>/dev/null) || label=
      [ -n "$label" ] || label=$(basename "$f" .plist)
      h=$("${SHA[@]}" <"$f" 2>/dev/null | awk '{print $1}')
      printf 'LAGENT\t%s\t%s\t%s\t%s\n' "$scope" "$(esc "$label")" "${h:--}" "$(esc "$f")"
    done
  done
fi

[ -n "$PROCS" ] && printf '%s\n' "$PROCS" | while IFS= read -r p; do printf 'PROC\t%s\n' "$(esc "$p")"; done
printf 'META\tscan_seconds\t%s\n' "$(( $(date +%s) - START ))"
}
main "$@"; exit $?

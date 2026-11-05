#!/usr/bin/env bash
# owners.sh - attributes changed paths to the package that installed them.
# stdin: one absolute path per line. stdout: OWN<TAB>manager<TAB>package<TAB>path.
# Read-only. Works with bash 3.2 (macOS) and 5.x.
main() {
export LC_ALL=C
export HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_ANALYTICS=1 HOMEBREW_NO_ENV_HINTS=1 NO_COLOR=1
export PATH="$PATH:/opt/homebrew/bin:/usr/local/bin"
TMP=$(mktemp -d 2>/dev/null || mktemp -d -t owners) || return 1
trap 'rm -rf "$TMP"' EXIT
have() { command -v "$1" >/dev/null 2>&1; }

cat >"$TMP/paths"
: >"$TMP/own"

# dpkg -S names the package that ships a path.
if have dpkg; then
  tr '\n' '\0' <"$TMP/paths" | xargs -0 dpkg -S 2>/dev/null | awk '
    /^diversion / {next}
    { i=index($0,": /"); if (i==0) next
      pk=substr($0,1,i-1); p=substr($0,i+2)
      split(pk,a,", "); k=a[1]; sub(/:[^:]*$/,"",k)
      printf "dpkg\t%s\t%s\n", k, p }' >>"$TMP/own"
fi

# Homebrew keeps each formula and cask under Cellar/<name>/ and Caskroom/<name>/.
awk '
  { p=$0 }
  match(p, /\/Cellar\/[^\/]+\//)    { printf "brew\t%s\t%s\n", substr(p,RSTART+8,RLENGTH-9), p; next }
  match(p, /\/Caskroom\/[^\/]+\//)  { printf "brew-cask\t%s\t%s\n", substr(p,RSTART+10,RLENGTH-11), p }
' "$TMP/paths" >>"$TMP/own"

# Links into a Cellar or a global node_modules belong to that formula or package.
n=0
while IFS= read -r p; do
  [ -L "$p" ] || continue
  n=$((n+1)); [ $n -le 500 ] || break
  t=$(readlink "$p" 2>/dev/null) || continue
  case "$t" in
    */Cellar/*) name=${t#*Cellar/}; name=${name%%/*}; printf 'brew\t%s\t%s\n' "$name" "$p" >>"$TMP/own";;
    */lib/node_modules/@*/*) name=$(printf '%s' "${t#*/lib/node_modules/}" | cut -d/ -f1,2); printf 'npm\t%s\t%s\n' "$name" "$p" >>"$TMP/own";;
    */lib/node_modules/*) name=${t#*/lib/node_modules/}; name=${name%%/*}; printf 'npm\t%s\t%s\n' "$name" "$p" >>"$TMP/own";;
  esac
done <"$TMP/paths"

# pip: a RECORD file lists every file of its distribution, relative to site-packages.
awk 'match($0, /^.*\/(site|dist)-packages\//) { print substr($0,1,RLENGTH-1) }' "$TMP/paths" | sort -u |
while IFS= read -r d; do
  for rec in "$d"/*.dist-info/RECORD; do
    [ -f "$rec" ] || continue
    awk -F, -v d="$d" -v f="$rec" '
      BEGIN { pk=f; sub(/\/RECORD$/,"",pk); sub(/.*\//,"",pk); sub(/-[^-]*\.dist-info$/,"",pk) }
      $1 !~ /^\.\./ && $1 != "" { printf "pip\t%s\t%s/%s\n", pk, d, $1 }' "$rec"
  done
done >>"$TMP/own"

# npm: global packages live under `npm root -g`, as <name> or @<scope>/<name>.
if have npm; then
  root=$(npm root -g 2>/dev/null)
  if [ -n "$root" ]; then
    awk -v r="$root/" 'index($0,r)==1 {
      rest=substr($0,length(r)+1); split(rest,a,"/"); n=a[1]
      if (n ~ /^@/) n=n "/" a[2]
      printf "npm\t%s\t%s\n", n, $0 }' "$TMP/paths" >>"$TMP/own"
  fi
fi

# Only paths that were asked about, the first manager that claims each.
awk -F'\t' -v P="$TMP/paths" '
  function esc(s,  o,i,c){ if (index(s,"\\")==0) return s; o=""
    for(i=1;i<=length(s);i++){c=substr(s,i,1); o=o (c=="\\"?"\\\\":c)} return o }
  FILENAME==P { want[$0]=1; next }
  ($3 in want) && !seen[$3]++ { printf "OWN\t%s\t%s\t%s\n", $1, $2, esc($3) }
' "$TMP/paths" "$TMP/own"
}
main "$@"; exit $?

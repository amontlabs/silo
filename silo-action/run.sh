#!/bin/sh
# Fork a Silo computer, run INPUT_RUN in it, summarise the change, fold it, drop the fork.
# Inputs arrive as INPUT_* variables (see action.yml). Needs python3 to render the
# summary, which a Mac with the command line tools has.

silo=${INPUT_SILO_PATH:-silo}
from=${INPUT_FROM:-main}
fold=${INPUT_FOLD:-pr}
tmp=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/silo-action.XXXXXX") || exit 1
trap 'rm -rf "$tmp"' EXIT

out() { [ -n "$GITHUB_OUTPUT" ] && printf '%s=%s\n' "$1" "$2" >> "$GITHUB_OUTPUT"; return 0; }
summary() { if [ -n "$GITHUB_STEP_SUMMARY" ]; then cat >> "$GITHUB_STEP_SUMMARY"; else cat > /dev/null; fi; }
fail() { echo "silo-action: $1" >&2; exit "${2:-1}"; }

case $fold in pr | request | none) ;; *) fail "fold must be pr, request or none, not '$fold'" ;; esac
[ -n "$INPUT_RUN" ] || fail "run is empty"

command -v "$silo" > /dev/null 2>&1 || fail "silo not found ($silo). This runner needs Silo installed."
"$silo" list > /dev/null 2>&1 || fail "the Silo engine does not answer ('$silo list' failed). Start Silo on this runner."

name="gha-${GITHUB_RUN_ID:-local}-${GITHUB_RUN_ATTEMPT:-1}"
out fork "$name"

# silo fork <source> --name N [--secret S]... -- sh -c "$run"
source=$from
if [ -n "$INPUT_RECIPE" ]; then
  echo "silo-action: silo up --recipe $INPUT_RECIPE"
  "$silo" up --recipe "$INPUT_RECIPE" || fail "silo up failed" $?
  source=main
fi
set -- fork "$source" --name "$name"
for s in $(printf '%s' "$INPUT_SECRETS" | tr ',' ' '); do
  set -- "$@" --secret "$s"
done

"$silo" "$@" -- sh -c "$INPUT_RUN"
code=$?
out exit-code "$code"

if ! "$silo" status "$name" > /dev/null 2>&1; then
  printf '### Silo\n\nThe fork `%s` was not made (exit %s).\n' "$name" "$code" | summary
  [ "$code" -ne 0 ] || code=1
  fail "fork $name was not made" "$code"
fi

# Review: the buckets and the credentials line, as Markdown.
"$silo" diff "$name" --json > "$tmp/diff.json" 2> "$tmp/diff.err"
diff_code=$?
{
  printf '### Silo: `%s`\n\nThe command exited with %s.\n\n' "$name" "$code"
  if [ "$diff_code" -eq 0 ] && python3 - "$tmp/diff.json" > "$tmp/diff.md" 2> /dev/null << 'PY'
import json, sys
d = json.load(open(sys.argv[1]))
buckets = [b for b in d.get("summary") or [] if b.get("lines")]
if not buckets:
    print("No changes.")
for b in buckets:
    print("**%s**\n" % b.get("bucket", "Other"))
    for line in b["lines"]:
        print("- `%s`" % line.replace("`", "'"))
    print()
creds = (d.get("not_kept") or {}).get("credentials") or {}
if creds:
    n = sum(len(v) for v in creds.values())
    print("Credentials touched in the fork (not kept): %d file(s) in %s" % (n, ", ".join("`%s`" % k for k in creds)))
PY
  then
    cat "$tmp/diff.md"
  else
    printf 'The change summary could not be read (`silo diff` exit %s).\n' "$diff_code"
    sed 's/^/    /' "$tmp/diff.err" 2> /dev/null
  fi
} | summary

# Reads a field from the fold's JSON: json_field FILE KEY...
json_field() {
  f=$1; shift
  python3 -c 'import json,sys
d=json.load(open(sys.argv[1]))
for k in sys.argv[2:]:
    v=d
    for p in k.split("."):
        v=v.get(p) if isinstance(v,dict) else None
    if v:
        print(v); break' "$f" "$@" 2> /dev/null
}

status=$code
if [ "$code" -ne 0 ]; then
  echo "silo-action: the command failed ($code); not folding"
elif [ "$fold" = pr ]; then
  # silo fold --pr reads GITHUB_TOKEN and SILO_GITHUB_API_URL from the environment.
  "$silo" fold "$name" --pr --json > "$tmp/fold.json" 2> "$tmp/fold.err"
  fold_code=$?
  if [ "$fold_code" -eq 0 ]; then
    pr_url=$(json_field "$tmp/fold.json" url prUrl pr_url pr.url)
    out pr-url "$pr_url"
    printf '\nPull request: %s\n' "${pr_url:-opened}" | summary
  else
    cat "$tmp/fold.err" >&2
    printf '\n`silo fold --pr` failed (exit %s).\n' "$fold_code" | summary
    status=$fold_code
  fi
elif [ "$fold" = request ]; then
  "$silo" fold "$name" --request --json > "$tmp/fold.json" 2> "$tmp/fold.err"
  fold_code=$?
  if [ "$fold_code" -eq 0 ]; then
    request_id=$(json_field "$tmp/fold.json" id request.id)
    out request-id "$request_id"
    printf '\nReview request: `%s`\n' "$request_id" | summary
  else
    cat "$tmp/fold.err" >&2
    printf '\n`silo fold --request` failed (exit %s).\n' "$fold_code" | summary
    status=$fold_code
  fi
fi

case $INPUT_KEEP_FORK in
  true | True | TRUE | 1 | yes) echo "silo-action: keeping fork $name" ;;
  *) "$silo" drop "$name" > /dev/null 2>&1 || echo "silo-action: could not drop $name" >&2 ;;
esac

exit "$status"

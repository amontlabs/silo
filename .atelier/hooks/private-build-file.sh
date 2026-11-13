#!/bin/sh
# Makes app/SiloUI/github-build.local.json equal to the main checkout's copy, so worktrees and
# the gate build with the owner's GitHub App configuration. Prints nothing on success and never
# prints the file; fails, naming the missing path, when the main checkout has no copy.
set -eu
cd "$(dirname "$0")/../.."
file=app/SiloUI/github-build.local.json
common=$(git rev-parse --path-format=absolute --git-common-dir)
main=$(dirname "$common")
source=$main/$file
if [ ! -f "$source" ]; then
  echo "private-build-file: $source is missing (docs/SiloUI-RELEASES.md#local-setup)" >&2
  exit 1
fi
[ "$(cd "$main" && pwd -P)" = "$(pwd -P)" ] && exit 0
cmp -s "$source" "$file" 2>/dev/null && exit 0
umask 077
temporary=$(mktemp "$file.XXXXXX")
trap 'rm -f "$temporary"' EXIT
cat "$source" >"$temporary"
mv -f "$temporary" "$file"
trap - EXIT

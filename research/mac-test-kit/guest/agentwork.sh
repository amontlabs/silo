#!/bin/sh
# Runs inside a macOS guest as the provisioned user: one step of agent-like work (test 15).
#   agentwork.sh STEP REPO...
# REPO is OWNER/NAME on GitHub. The first is installed with npm and its tests run; the others are
# only fetched. With git (the Command Line Tools) a repository is cloned, else its tarball is
# fetched. Then file churn as a build makes: 20,000 small files, 10% of them rewritten, 50 build
# outputs of 4 MB, 20,000 rows of a 200,000-row SQLite table updated. The step from two steps
# before is deleted, as an agent's old worktrees are.
set -u
k="$1"; shift
export PATH="$HOME/node/bin:$PATH"
W="$HOME/work"; mkdir -p "$W"; cd "$W" || exit 1
rm -rf "s$((k - 2))"
mkdir -p "s$k"; cd "s$k" || exit 1
first=1
for repo in "$@"; do
  name="$(basename "$repo")"
  if git --version >/dev/null 2>&1; then
    git clone -q --depth 1 "https://github.com/$repo" "$name" || exit 1
  else
    mkdir -p "$name" && curl -fsSL "https://github.com/$repo/archive/HEAD.tar.gz" | tar -xzf - -C "$name" --strip-components 1 || exit 1
  fi
  if [ "$first" = 1 ] && command -v npm >/dev/null 2>&1; then
    (cd "$name" && npm install --no-audit --no-fund --loglevel=error >/dev/null 2>&1) || exit 1
    (cd "$name" && npm test >/dev/null 2>&1) || true
  fi
  first=0
done
perl -e '
  use File::Path qw(make_path);
  my $d = "churn"; my $buf = "s" x 8192;
  for my $i (0 .. 19999) {
    my $sub = sprintf("%s/%03d", $d, int($i / 200)); make_path($sub) unless $i % 200;
    open(my $f, ">", "$sub/f$i.js") or die; print $f substr($buf, 0, 200 + int(rand(7000))); close $f;
  }
  for (my $i = 0; $i < 20000; $i += 10) {
    my $p = sprintf("%s/%03d/f%d.js", $d, int($i / 200), $i);
    open(my $f, ">", $p) or die; print $f "rewritten " x 100; close $f;
  }' || exit 1
i=1
while [ "$i" -le 50 ]; do dd if=/dev/urandom of="$HOME/obj$i.o" bs=1048576 count=4 2>/dev/null; i=$((i + 1)); done
sqlite3 "$HOME/app.db" "
  create table if not exists t(id integer primary key, v blob);
  insert or ignore into t select x, randomblob(400) from (with recursive c(x) as (select 1 union all select x + 1 from c where x < 200000) select x from c);
  with recursive c(n) as (select 1 union all select n + 1 from c where n < 20000)
    update t set v = randomblob(400) where id in (select abs(random()) % 200000 + 1 from c);" || exit 1
sync

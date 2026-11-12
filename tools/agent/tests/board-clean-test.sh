#!/usr/bin/env bash
# Tests the build state directory defaults of board/clean, clean's dry run, and clean --merged, in
# temporary git repos. No cargo, node or engine is started.
set -uo pipefail
A=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
W=$(mktemp -d "${TMPDIR:-/tmp}/boardclean-test.XXXXXX"); W=$(cd "$W" && pwd -P); trap 'rm -rf "$W"' EXIT
export TMPDIR=$W/tmp; mkdir -p "$TMPDIR"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
unset BOARD BOARD_INTEGRATION SILO_BUILD_STATE SILO_INTEGRATION_BRANCH
N=0; BAD=0
ok() { N=$((N+1)); if [ "$2" = 0 ]; then echo "PASS $1"; else echo "FAIL $1 ${3:-}"; BAD=$((BAD+1)); fi; }
has() { grep -qF -- "$2" <<<"$1"; }

R=$W/repo; mkdir -p $R; git -C $R init -q -b main; echo a >$R/f; git -C $R add -A; git -C $R commit -qm init
mkbranch() { # name [commit] -> a worktree at $W/<name>
  git -C $R worktree add -q -b $1 $W/$1 main
  [ "${2:-}" = nocommit ] || { echo $1 >$W/$1/$1.txt; git -C $W/$1 add -A; git -C $W/$1 commit -qm $1; }
}
merge() { git -C $R merge -q --no-ff -m "merge $1" $1; }

# --- state directory defaults
cd $R
$A/board add x >/dev/null 2>&1; rc=$?
ok "no env: board writes next to the main checkout (<checkout>.state/board.tsv)" $([ $rc = 0 ] && [ -f $R.state/board.tsv ] && echo 0 || echo 1)
SILO_BUILD_STATE=$W/st $A/board add y >/dev/null 2>&1
ok "SILO_BUILD_STATE wins over the default" $([ -f $W/st/board.tsv ] && echo 0 || echo 1)
BOARD=$W/b.tsv SILO_BUILD_STATE=$W/st $A/board add z >/dev/null 2>&1
ok "BOARD wins over SILO_BUILD_STATE" $([ -f $W/b.tsv ] && echo 0 || echo 1)
git -C $R switch -q -c direction
git -C $R worktree add -q --detach $W/other main 2>/dev/null; git -C $R switch -q main
git -C $R worktree add -q $W/dir-co direction
cd $W/other
BOARD_INTEGRATION=direction $A/board add w >/dev/null 2>&1
ok "BOARD_INTEGRATION picks the integration checkout, even from another worktree" $([ -f $W/dir-co.state/board.tsv ] && echo 0 || echo 1)
out=$(BOARD_INTEGRATION=nosuch $A/board add v 2>&1); rc=$?
ok "an integration branch that is not checked out is refused" $([ $rc != 0 ] && has "$out" "not checked out" && echo 0 || echo 1 "$out")
git -C $R worktree remove --force $W/other; git -C $R worktree remove $W/dir-co; git -C $R branch -q -D direction
rm -rf $R.state $W/st $W/dir-co.state $W/b.tsv; cd $R

# --- clean <item> dry run says what the real run does
export BOARD=$W/board.tsv
mkbranch item1; merge item1
$A/board add item1 >/dev/null; $A/board set item1 state=merged worktree=$W/item1
out=$($A/clean item1 2>&1); rc=$?
ok "dry run: removes worktree and deletes the branch (no 'keep branch: checked out')" $([ $rc = 0 ] && has "$out" "remove worktree" && has "$out" "delete branch item1" && ! has "$out" "keep branch" && [ -d $W/item1 ] && echo 0 || echo 1 "$out")
out=$($A/clean item1 --yes 2>&1)
ok "real run does what the dry run said" $([ ! -e $W/item1 ] && ! git -C $R rev-parse -q --verify refs/heads/item1 >/dev/null && [ "$($A/board get item1 state)" = cleaned ] && echo 0 || echo 1 "$out")
mkbranch item2; merge item2; echo dirty >$W/item2/untracked
$A/board add item2 >/dev/null; $A/board set item2 state=merged worktree=$W/item2
out=$($A/clean item2 2>&1); rc=$?
ok "dry run refuses a worktree with local changes, as the real run does" $([ $rc != 0 ] && has "$out" "local changes" && echo 0 || echo 1 "$out")
out=$($A/clean item2 --yes 2>&1); rc=$?
ok "real run refuses it too and removes nothing" $([ $rc != 0 ] && [ -d $W/item2 ] && git -C $R rev-parse -q --verify refs/heads/item2 >/dev/null && echo 0 || echo 1 "$out")
git -C $R worktree remove --force $W/item2; git -C $R branch -q -D item2

# --- clean --merged
mkbranch m1; merge m1; head -c 300000 /dev/zero >$W/m1/big.bin; git -C $W/m1 check-ignore -q big.bin || echo big.bin >>$R/.git/info/exclude
mkbranch m2; merge m2                                   # merged, then changed (untracked)
echo wip >$W/m2/wip.txt
mkbranch m3                                              # not merged
mkbranch m4 nocommit                                     # fresh branch with no commit
mkbranch m5; merge m5; git -C $W/m5 checkout -q --detach  # detached
mkbranch m6; merge m6; git -C $R worktree lock $W/m6     # locked
mkbranch m7; merge m7; mkbranch m8 nocommit
mkbranch m9; merge m9
rm -f $BOARD; $A/board add m7 >/dev/null; $A/board set m7 state=merged
$A/board add m8 --depends m7 >/dev/null; $A/board set m8 state=running worktree=$W/m8
$A/board add m1 >/dev/null; $A/board set m1 state=merged
git -C $R worktree add -q $W/repo.suite -b suiteb main >/dev/null 2>&1
SHA_M3=$(git -C $R rev-parse m3)
out=$($A/clean --merged 2>&1); rc=$?
ok "dry run: lists removals and keeps, deletes nothing" $([ $rc = 0 ] && has "$out" "remove worktree $W/m1" && has "$out" "keep $W/m2" && [ -d $W/m1 ] && git -C $R rev-parse -q --verify refs/heads/m1 >/dev/null && echo 0 || echo 1 "$out")
out=$($A/clean --merged --yes 2>&1); rc=$?
echo "$out" | sed 's/^/    /'
gone() { [ ! -e $W/$1 ] && ! git -C $R rev-parse -q --verify refs/heads/$1 >/dev/null; }
kept() { [ -e $W/$1 ] && git -C $R rev-parse -q --verify refs/heads/$1 >/dev/null; }
ok "merged and clean worktrees are removed with their branches" $(gone m1 && gone m9 && echo 0 || echo 1)
ok "a merged worktree with untracked files is kept" $(kept m2 && has "$out" "$W/m2" && has "$out" "local changes" && echo 0 || echo 1)
ok "an unmerged branch is kept" $(kept m3 && echo 0 || echo 1)
ok "a fresh branch without commits is kept" $(kept m4 && has "$out" "no commit of its own" && echo 0 || echo 1)
ok "a detached worktree is kept and listed" $([ -e $W/m5 ] && has "$out" "detached HEAD" && echo 0 || echo 1)
ok "a locked worktree is kept" $(kept m6 && echo 0 || echo 1)
ok "a merged branch an unmerged board item depends on is kept" $(kept m7 && has "$out" "depends on it" && echo 0 || echo 1)
ok "the main, integration and .suite checkouts are untouched" $([ -d $R/.git ] && [ -d $W/repo.suite ] && git -C $R rev-parse -q --verify refs/heads/main >/dev/null && echo 0 || echo 1)
ok "board items of removed branches become cleaned; others stay" $([ "$($A/board get m1 state)" = cleaned ] && [ "$($A/board get m7 state)" = merged ] && echo 0 || echo 1)
ok "it reports the space freed" $(has "$out" "freed" && has "$out" "2 worktree(s) removed" && echo 0 || echo 1)
ok "an unmerged branch's commits are intact" $([ "$(git -C $R rev-parse m3)" = $SHA_M3 ] && echo 0 || echo 1)
out=$($A/clean --merged m1 2>&1); ok "--merged and an item together are refused" $([ $? != 0 ] && echo 0 || echo 1)

echo "$N checks, $BAD failed"; [ $BAD = 0 ]

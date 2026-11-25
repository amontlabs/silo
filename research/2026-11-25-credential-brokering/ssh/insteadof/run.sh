#!/bin/sh
# Which Git clients follow Git's url.<base>.insteadOf, the rewrite of git@github.com: to HTTPS.
# Needs network to github.com over HTTPS, Go and cargo. The rewrite sits in a HOME's .gitconfig.
set -u
T=${1:-/tmp/insteadof}; HERE=$(cd "$(dirname "$0")" && pwd)
rm -rf "$T"; mkdir -p "$T/home" "$T/gogit" "$T/cargo/src"
printf '[url "https://github.com/"]\n\tinsteadOf = git@github.com:\n\tinsteadOf = ssh://git@github.com/\n' > "$T/home/.gitconfig"
export HOME="$T/home" SSH_AUTH_SOCK=
echo "== git (CLI)"; GIT_TRACE=1 git ls-remote git@github.com:octocat/Hello-World.git HEAD 2>&1 | grep -E 'remote-https|HEAD$' | head -2
echo "== go-git v5.16.2"; cp "$HERE/gogit.go" "$T/gogit/main.go"
(cd "$T/gogit" && go mod init gg >/dev/null 2>&1 && go get github.com/go-git/go-git/v5@v5.16.2 >/dev/null 2>&1 && go mod tidy >/dev/null 2>&1 && go build -o gg . && ./gg "$T/gogit/out")
echo "== cargo (libgit2)"
printf '[package]\nname="t"\nversion="0.1.0"\nedition="2021"\n[dependencies]\nhw={git="ssh://git@github.com/octocat/Hello-World.git"}\n' > "$T/cargo/Cargo.toml"
echo 'fn main(){}' > "$T/cargo/src/main.rs"
(cd "$T/cargo" && CARGO_HOME="$T/cargohome" cargo fetch 2>&1 | tail -2)

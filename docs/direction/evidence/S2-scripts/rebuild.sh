#!/bin/zsh
# Prototype of the rebuild, by name: rebuild.sh <old> <new-temp-name>
zmodload zsh/datetime; . ~/.s2/env.sh; M=$MSB_PATH; S=${0:A:h}; OLD=$1; NEW=$2; H=$MSB_HOME/sandboxes
t(){ local s=$EPOCHREALTIME; "$@"; printf '    [%s: %.1fs]\n' "$LABEL" $((EPOCHREALTIME-s)); }
step(){ LABEL=$1; shift; echo "== $LABEL"; t "$@"; }
OLDID=$($M inspect $OLD --format json | python3 -c "import json,sys;print(json.load(sys.stdin)['config']['labels']['silo.machine-id'])")
echo old id $OLDID
step 1-stop-old $M stop $OLD >/dev/null 2>&1
step 2-checkpoint-old $M snapshot create before-rebuild --from-sandbox $OLD --group g-$OLD --integrity
step 3-create-new $M create ghcr.io/amontlabs/silo-guest:ubuntu-24.04-v5-arm64 --pull never --name $NEW --cpus 1 --max-cpus 1 --memory 1G --max-memory 1G --root-disk 4G --mount-owned /workspace:kind=disk,size=1G --label silo.managed=true --label silo.machine-id=$NEW-tmp --net-strict=true --no-start --quiet
step 4-first-boot+account $M start $NEW >/dev/null
step 5-account-setup $S/account.sh $NEW
step 6-stop-new $M stop $NEW >/dev/null
step 7-swap-workspace python3 -I $S/swap-workspace.py $H/$OLD $H/$NEW
step 8-start-old-for-home $M start $OLD >/dev/null
step 9-home-tar-in-old $M exec $OLD --no-tty --user root -- sh -c 'tar -C /home -cpf /tmp/home.tar --sparse --xattrs --acls --numeric-owner silo 2>&1 | grep -v "socket ignored"; true'
step 10-cp-to-host $M cp $OLD:/tmp/home.tar $HOME/.s2/stage/home.tar
step 11-start-new $M start $NEW >/dev/null
step 12-cp-to-new $M cp $HOME/.s2/stage/home.tar $NEW:/tmp/home.tar
step 13-extract-home $M exec $NEW --no-tty --user root -- sh -c 'tar -C /home -xpf /tmp/home.tar --xattrs --acls --numeric-owner --same-owner && rm /tmp/home.tar && sync'
step 14-recipe-replay-apt $M exec $NEW --no-tty --user root -- sh -c 'apt-get update -q >/dev/null 2>&1; DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends tree >/dev/null 2>&1; which tree'
step 15-stop-both sh -c "$M stop $OLD >/dev/null; $M stop $NEW >/dev/null"
step 16-rename-into-place sh -c "$M rename $OLD $OLD-undo --quiet && $M rename $NEW $OLD --quiet && $M modify $OLD-undo --label silo.machine-id=$OLDID-undo >/dev/null && $M modify $OLD --label silo.machine-id=$OLDID >/dev/null"
echo "== labels now"; for n in $OLD $OLD-undo; do $M inspect $n --format json | python3 -c "import json,sys;d=json.load(sys.stdin)['config'];print(d['name'],d['labels']['silo.machine-id'],d['image']['Oci']['reference'][-22:])"; done
rm -f $HOME/.s2/stage/home.tar

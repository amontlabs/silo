#!/bin/bash
# Runs inside the outer user namespace of userns.py: mounts the container's overlay root, then crun.
# The lower layers, top first: the Silo boot layer (init shim and test scripts), the unpacked guest image.
# userxattr is how an overlay is mounted without privilege in a user namespace.
d=$1 name=$2 boot=$3 base=$4
mount --make-rprivate / || exit 90
mount -t overlay overlay -o "lowerdir=$boot:$base,upperdir=$d/upper,workdir=$d/work,userxattr" "$d/merged" || exit 91
exec crun --root "$d/state" --cgroup-manager=cgroupfs run --bundle "$d/bundle" --pid-file "$d/pid" "$name"

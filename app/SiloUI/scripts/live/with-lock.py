#!/usr/bin/env python3
"""with-lock.py [--dir DIR] [--info TEXT] NAME... -- COMMAND [ARG...]

Takes an exclusive, non-blocking flock on DIR/<NAME>.lock for every NAME, then execs COMMAND with the locks
still held: the lock files stay open (inheritable) in the command and in whatever it starts, so each lock is
released when the last of those processes ends, however it ends. The holder's TEXT is written into each
lock file. When a lock is held elsewhere, prints who holds it and exits 3.
flock(2) works the same on macOS and Linux; util-linux's flock(1) is not on macOS.
"""
import errno
import fcntl
import os
import sys

DEFAULT_DIR = os.path.join(os.path.expanduser("~"), ".local", "state", "silo-live")


def main(argv):
    lock_dir = os.environ.get("LIVE_LOCK_DIR") or DEFAULT_DIR
    info = ""
    args = argv[1:]
    while args and args[0] in ("--dir", "--info"):
        if len(args) < 2:
            print("with-lock: %s needs a value" % args[0], file=sys.stderr)
            return 2
        if args[0] == "--dir":
            lock_dir = args[1]
        else:
            info = args[1]
        args = args[2:]
    if "--" not in args:
        print("usage: with-lock.py [--dir DIR] [--info TEXT] NAME... -- COMMAND [ARG...]", file=sys.stderr)
        return 2
    split = args.index("--")
    names, command = args[:split], args[split + 1:]
    if not names or not command:
        print("usage: with-lock.py [--dir DIR] [--info TEXT] NAME... -- COMMAND [ARG...]", file=sys.stderr)
        return 2
    os.makedirs(lock_dir, mode=0o700, exist_ok=True)
    held = []
    for name in sorted(set(names)):
        if "/" in name or name.startswith("."):
            print("with-lock: bad lock name: %s" % name, file=sys.stderr)
            return 2
        path = os.path.join(lock_dir, name + ".lock")
        fd = os.open(path, os.O_RDWR | os.O_CREAT, 0o600)
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError as err:
            if err.errno not in (errno.EWOULDBLOCK, errno.EAGAIN, errno.EACCES):
                raise
            holder = os.read(fd, 4096).decode("utf-8", "replace").strip() or "unknown holder"
            print("busy: %s is held (%s): %s" % (name, path, holder), file=sys.stderr)
            return 3
        os.ftruncate(fd, 0)
        os.write(fd, (info + "\n").encode())
        os.set_inheritable(fd, True)
        held.append(fd)
    os.execvp(command[0], command)


if __name__ == "__main__":
    sys.exit(main(sys.argv))

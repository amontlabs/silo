"""Add one Include line at the top of an SSH config file, and take it out again, exactly.

    08-sshconfig.py install --config PATH --line TEXT --state STATE --backup BACKUP
    08-sshconfig.py restore --state STATE
    08-sshconfig.py check --config PATH --line TEXT   (exit 0 when LINE is the file's first line)

install: when PATH is a symbolic link (a dotfiles manager's), the file it points at is the one
changed. The file's bytes go to BACKUP (with its mode and times), the state of everything to
STATE, then the file becomes LINE, a newline, and the old bytes. The file is written in place,
so its inode, owner, mode, ACLs and extended attributes stay. When there is no file, it is made
with mode 0600 (and ~/.ssh with 0700 when missing); restore removes what was made.

restore: puts the old bytes back and the old modification time, then compares the file with
BACKUP byte for byte. When the file changed after install (an app or a person wrote to it), it
keeps that change and only takes out the line, then says so. Prints one JSON object.
"""
import argparse
import hashlib
import json
import os
import shutil
import sys


def sha(b):
    return hashlib.sha256(b).hexdigest()


def read(path):
    with open(path, "rb") as f:
        return f.read()


def write_in_place(path, data):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT, 0o600)
    try:
        os.ftruncate(fd, 0)
        view = memoryview(data)
        while view:
            n = os.write(fd, view)
            view = view[n:]
        os.fsync(fd)
    finally:
        os.close(fd)


def install(a):
    line = a.line.encode() + b"\n"
    path = os.path.expanduser(a.config)
    real = os.path.realpath(path)
    st = {"config": path, "real": real, "was_symlink": os.path.islink(path),
          "line": a.line, "backup": a.backup, "existed": os.path.exists(real)}
    ssh_dir = os.path.dirname(real)
    st["dir_created"] = not os.path.isdir(ssh_dir)
    if st["dir_created"]:
        os.makedirs(ssh_dir, mode=0o700)
    if st["existed"]:
        old = read(real)
        s = os.stat(real)
        st.update(mode=s.st_mode & 0o7777, atime=s.st_atime, mtime=s.st_mtime, sha256=sha(old), size=len(old))
        shutil.copy2(real, a.backup)
        if read(a.backup) != old:
            raise SystemExit("the backup is not the same as the file: nothing changed")
    else:
        old = b""
        st.update(sha256=None, size=0)
    new = line + old
    st["written_sha256"] = sha(new)
    with open(a.state, "w") as f:
        json.dump(st, f)
    write_in_place(real, new)
    if not st["existed"]:
        os.chmod(real, 0o600)
    print(json.dumps({"installed": True, "real": real, "was_symlink": st["was_symlink"],
                      "existed": st["existed"], "size_before": st["size"]}))


def restore(a):
    st = json.load(open(a.state))
    real, line = st["real"], st["line"].encode() + b"\n"
    r = {"restored": False, "how": "", "verified": False}
    cur = read(real) if os.path.exists(real) else None
    if cur is None:
        r["how"] = "the file was gone"
    elif sha(cur) == st["written_sha256"]:
        r["how"] = "unchanged since install"
        if st["existed"]:
            write_in_place(real, read(st["backup"]))
            os.chmod(real, st["mode"])
            os.utime(real, (st["atime"], st["mtime"]))
        else:
            os.unlink(real)
            if st["dir_created"]:
                try:
                    os.rmdir(os.path.dirname(real))
                except OSError:
                    pass
        r["restored"] = True
    else:
        keep = st["backup"] + ".changed-during-test"
        with open(keep, "wb") as f:
            f.write(cur)
        if cur.startswith(line):
            rest = cur[len(line):]
            r["how"] = "changed during the test; only the Include line was taken out (the changed file is kept in %s)" % keep
        elif line in cur:
            i = cur.index(line)
            if i > 0 and cur[i - 1:i] != b"\n":
                rest = None
            else:
                rest = cur[:i] + cur[i + len(line):]
            r["how"] = "the Include line had moved; it was taken out where it was (the changed file is kept in %s)" % keep
        else:
            rest = None
            r["how"] = "the Include line is no longer in the file; nothing written (the file is kept in %s)" % keep
        if rest is not None:
            if not st["existed"] and not rest:
                os.unlink(real)
            else:
                write_in_place(real, rest)
            r["restored"] = True
    if st["existed"]:
        now = read(real) if os.path.exists(real) else None
        r["verified"] = now is not None and now == read(st["backup"]) and sha(now) == st["sha256"]
        r["mode_same"] = now is not None and (os.stat(real).st_mode & 0o7777) == st["mode"]
    else:
        r["verified"] = not os.path.exists(real)
    r["was_symlink"] = st["was_symlink"]
    if st["was_symlink"]:
        r["symlink_kept"] = os.path.islink(st["config"]) and os.path.realpath(st["config"]) == real
    print(json.dumps(r))
    return 0 if r["verified"] else 1


def check(a):
    path = os.path.expanduser(a.config)
    try:
        first = read(path).split(b"\n", 1)[0].decode()
    except OSError:
        return 1
    return 0 if first == a.line else 1


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["install", "restore", "check"])
    ap.add_argument("--config")
    ap.add_argument("--line")
    ap.add_argument("--state")
    ap.add_argument("--backup")
    a = ap.parse_args()
    return {"install": install, "restore": restore, "check": check}[a.cmd](a) or 0


if __name__ == "__main__":
    sys.exit(main())

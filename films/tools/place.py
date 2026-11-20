"""place.py: where a film's tools may read and write. Standard library only.

`film(arg)` gives the film folder named by arg, resolved, and stops unless it is a direct child of films/ (W)
that holds a film.json: a folder elsewhere, or reached through a link that leaves films/, is refused.
`clip(ch, name)` gives the path of a clip named in voice.json, and stops unless the name is `voice/<file>`
with a plain file name and the file resolves inside that film's voice/.
`inside(ch, sub)` gives <film>/<sub> and stops if it is a link that leaves the film folder.
"""
import pathlib, sys

W = pathlib.Path(__file__).resolve().parent.parent
NOT_FILMS = {"kit", "player", "tools"}


def film(arg):
    ch = pathlib.Path(arg).resolve()
    if ch.parent != W or ch.name in NOT_FILMS or ch.name.startswith(".") or not (ch / "film.json").is_file():
        sys.exit(f"{arg}: not a film. A film is a folder directly under {W} that holds a film.json.")
    return ch


def inside(ch, sub):
    p = ch / sub
    if p.exists() and p.resolve() != p:
        sys.exit(f"{p} leads outside the film folder: refused.")
    return p


def clip(ch, name):
    parts = name.split("/")
    if len(parts) != 2 or parts[0] != "voice" or parts[1] in ("", ".", "..") or "\\" in name:
        sys.exit(f"voice.json names the clip {name!r}: a clip is voice/<file name>, inside the film's voice/.")
    voice = inside(ch, "voice")
    p = (voice / parts[1]).resolve()
    if p.parent != voice:
        sys.exit(f"voice.json names the clip {name!r}, which leads outside {voice}: refused.")
    return p

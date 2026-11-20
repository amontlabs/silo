"""Imported first by every Python tool here: runs the tool with W/.venv/bin/python (which has Kokoro's misaki),
offline (HF_HUB_OFFLINE=1), whichever python started it."""
import os, pathlib, sys
VENV = pathlib.Path(__file__).resolve().parent.parent / ".venv" / "bin" / "python"
os.environ["HF_HUB_OFFLINE"] = "1"
if os.environ.get("FILM_IN_VENV") != "1":
    if not VENV.exists():
        sys.exit(f"{VENV} is missing: make it as the README says, then run again.")
    os.environ["FILM_IN_VENV"] = "1"
    os.execv(str(VENV), [str(VENV), *sys.argv])

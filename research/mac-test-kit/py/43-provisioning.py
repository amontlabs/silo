"""For test 43: which options Apple's guest provisioning takes, read from the SDK's headers.

    python3 -I py/43-provisioning.py HEADER...

Reads every @interface of a class whose name ends in ProvisioningOptions and its @property
lines, and prints one JSON object: the classes and their properties, and whether any property
looks like a way to run something at the computer's first start (a script, a command, a
program, a package, a launch item, a file to copy in). If one does, Silo's relay could start
with the computer and #382's way in before the relay would need no road at all.
"""
import json
import re
import sys

RUNS = re.compile(r"script|command|program|executable|launch|package|install|payload|file|url|agent|daemon|run", re.I)


def read(paths):
    classes = {}
    for p in paths:
        try:
            text = open(p, errors="replace").read()
        except OSError:
            continue
        for m in re.finditer(r"@interface\s+(\w*ProvisioningOptions)\b(.*?)@end", text, re.S):
            props = classes.setdefault(m.group(1), [])
            for pm in re.finditer(r"@property\s*(\([^)]*\))?\s*([^;]+);", m.group(2)):
                decl = " ".join(pm.group(2).split())
                # Drop availability and nullability macros (API_AVAILABLE(macos(27.1)), NS_SWIFT_NAME(x)).
                bare = re.sub(r"\b[A-Z][A-Z0-9_]{2,}\s*(\((?:[^()]|\([^()]*\))*\))?", " ", decl)
                ids = re.findall(r"\w+", bare)
                props.append({"decl": decl, "name": ids[-1] if ids else decl})
    return classes


def main():
    classes = read(sys.argv[1:])
    found = [c + "." + p["name"] for c, ps in sorted(classes.items()) for p in ps if RUNS.search(p["name"])]
    print(json.dumps({"classes": {c: [p["name"] for p in ps] for c, ps in sorted(classes.items())},
                      "runs_program": bool(found), "candidates": found}, sort_keys=True))
    return 0 if classes else 1


if __name__ == "__main__":
    sys.exit(main())

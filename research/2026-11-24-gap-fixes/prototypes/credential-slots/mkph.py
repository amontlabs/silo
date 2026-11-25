"""Make a placeholder for a real value: the same length, the same public prefix (so a program that
checks the format accepts it), every other character random from the same class."""
import re, secrets, string, sys

CLASSES = [string.ascii_lowercase, string.ascii_uppercase, string.digits]


def public_prefix(real):
    # Known formats keep their prefix: ghp_, github_pat_, sk-ant-api03-, sk-proj-, npm_, pypi-, AKIA, xoxb-
    m = re.match(r"(github_pat_|sk-ant-[a-z]+\d*-|sk-proj-|sk-|gh[pousr]_|glpat-|npm_|pypi-|xox[bpa]-|hf_|AKIA|ASIA|AIza)", real)
    return m.group(1) if m else ""


def placeholder(real):
    p = public_prefix(real)
    out = []
    for c in real[len(p):]:
        cls = next((k for k in CLASSES if c in k), None)
        out.append(secrets.choice(cls) if cls else c)
    ph = p + "".join(out)
    return ph if ph != real else placeholder(real)


if __name__ == "__main__":
    print(placeholder(sys.stdin.read().strip()))

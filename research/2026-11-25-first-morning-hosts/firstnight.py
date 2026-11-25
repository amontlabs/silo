"""How many "host seen for the first time" alerts the first morning opens on, and where the paste
site lands among them, under each way of counting a host as already seen.

The first night is #373's and #382's: Claude Code works in ~/work/app (a Node app), Codex in
~/work/api, and Codex sends a 2.1 MB log to paste.rs. It is built from connections that were
measured, never invented, except the paste upload, which is the scenario's and is not sent:

  data/setup.jsonl   measured here: the downloads the first run's installers make, fetched through
                     rig/hostlog.py (the Claude Code installer's files, Homebrew, tmux and its four
                     dependencies from Homebrew's bottles, Codex's cask, the provider check)
  npm.jsonl          #350 (gap-fixes G1): `npm install` of a 21-dependency web app, then
                     `npx playwright install chromium`, with the process that opened each connection
  pip.jsonl          #350: `pip install` of 20 packages, a spaCy model, NLTK data, Playwright's Firefox
  docs.json          #350: 20 real documentation pages for one task, with the hosts of their links;
                     the agent reads each and follows two links (seeded, as #350's count_asks.py)

Hosts that no run here could reach, taken from what their vendors document or earlier drafts
measured, are marked as such in the output:

  Apple's Command Line Tools     swscan.apple.com, swcdn.apple.com (Apple's software update
                                 catalogue and its download host; not measured: no Mac here)
  Codex at start                 github.com, api.github.com, chatgpt.com, ab.chatgpt.com
                                 (#371, measured on Linux with Codex 0.172.0)
  Claude Code, signed in with a  api.anthropic.com, platform.claude.com, mcp-proxy.anthropic.com,
  Max plan, interactive          downloads.claude.ai, raw.githubusercontent.com,
                                 http-intake.logs.us5.datadoghq.com, browser-intake-us5-datadoghq.com
                                 (code.claude.com/docs/en/network-config, read 25 November 2026)

  python3 firstnight.py DIR_OF_350_DATA [SEEDS]
"""
import json
import os
import random
import sys

from publicsuffixlist import PublicSuffixList

HERE = os.path.dirname(os.path.abspath(__file__))
PSL = PublicSuffixList()
GET_UP = 864  # median bytes sent per connection of the measured setup downloads (31 connections)

CLT = ["swscan.apple.com", "swcdn.apple.com"]
CODEX_START = ["github.com", "api.github.com", "chatgpt.com", "ab.chatgpt.com"]
CLAUDE_START = ["api.anthropic.com", "platform.claude.com", "mcp-proxy.anthropic.com",
                "downloads.claude.ai", "raw.githubusercontent.com",
                "http-intake.logs.us5.datadoghq.com", "browser-intake-us5-datadoghq.com"]
# What each vendor documents its program reaches: the list Silo would ship with each agent program
# it offers. Claude Code's from its network page; Codex's from #371's measurement plus its API host.
PROGRAM_HOSTS = {
    "claude": set(CLAUDE_START) | {"claude.ai", "claude.com", "code.claude.com", "sso.claude.com",
                                   "storage.googleapis.com", "registry.npmjs.org", "github.com",
                                   "formulae.brew.sh", "bridge.claudeusercontent.com"},
    "codex": set(CODEX_START) | {"api.openai.com", "auth.openai.com"},
}
# #350's shipped sets, unchanged: hosts package install scripts download from, and documentation
# sites chosen before its walk ran.
sys.path.insert(0, os.path.join(HERE, "..", "2026-11-24-gap-fixes", "prototypes", "holds"))
from count_asks import DAY_ONE, DOCS, DOWNLOADS, SOCIAL  # noqa: E402

PASTE = ("paste.rs", 2_100_000, 1_200)


def domain(h):
    return PSL.privatesuffix(h) or h


def setup_events():
    out = []
    for line in open(os.path.join(HERE, "data", "setup.jsonl")):
        r = json.loads(line)
        out.append({"host": r["host"], "up": r["up"], "down": r["down"], "who": r["phase"], "src": "measured"})
    for h in CLT:
        out.append({"host": h, "up": GET_UP, "down": 0, "who": "setup-command-line-tools", "src": "Apple, not measured"})
    return out


def night_events(d, seed):
    ev = []
    for h in CLAUDE_START:
        ev.append({"host": h, "up": GET_UP, "down": 0, "who": "claude start", "src": "documented"})
    for h in CODEX_START:
        ev.append({"host": h, "up": GET_UP, "down": 0, "who": "codex start", "src": "#371"})
    for name, who in (("npm.jsonl", "claude: npm install"), ("pip.jsonl", "codex: pip install")):
        for line in open(os.path.join(d, name)):
            r = json.loads(line)
            ev.append({"host": r["host"], "up": GET_UP, "down": 0, "who": who, "src": "#350"})
    rnd = random.Random(seed)
    for p in json.load(open(os.path.join(d, "docs.json"))):
        host = p.get("final_host") or p["url"].split("/")[2]
        ev.append({"host": host, "up": GET_UP, "down": 0, "who": "claude: reads docs", "src": "#350"})
        links = sorted(h for h in p.get("link_hosts", {}) if h != host and h not in SOCIAL)
        for h in rnd.sample(links, min(2, len(links))):
            ev.append({"host": h, "up": GET_UP, "down": 0, "who": "claude: follows a link", "src": "#350"})
    ev.append({"host": "github.com", "up": 180_000, "down": 4_000, "who": "claude: git push", "src": "scenario"})
    ev.insert(len(ev) * 2 // 3, {"host": PASTE[0], "up": PASTE[1], "down": PASTE[2], "who": "codex: curl --data-binary @log",
                                 "src": "scenario, not sent"})
    return ev


def first_seen(events, seen):
    """{host: {"up": bytes sent this night, "who": first opener}} of hosts not in seen, in order."""
    new = {}
    for e in events:
        h = e["host"]
        if h in seen:
            continue
        n = new.setdefault(h, {"up": 0, "who": e["who"], "src": e["src"]})
        n["up"] += e["up"]
    return new


def rank(new, host, key):
    order = sorted(new, key=key)
    return order.index(host) + 1 if host in new else None


def main(d, seeds):
    setup = setup_events()
    setup_hosts = {e["host"] for e in setup}
    measured_setup = {e["host"] for e in setup if e["src"] == "measured"}
    programs = PROGRAM_HOSTS["claude"] | PROGRAM_HOSTS["codex"]
    ways = [
        ("A. every host is new (the property as it reads)", set()),
        ("B. hosts the first run's installers reached count as seen", setup_hosts),
        ("C. B, plus the hosts each agent program's vendor documents", setup_hosts | programs),
        ("D. C, plus #350's registries, download and documentation sets", setup_hosts | programs | DAY_ONE | DOWNLOADS | DOCS),
    ]
    print("Setup reached %d hosts (%d measured here, %d Apple's, not measured): %s"
          % (len(setup_hosts), len(measured_setup), len(setup_hosts - measured_setup), ", ".join(sorted(setup_hosts))))
    rows = []
    for seed in seeds:
        night = night_events(d, seed)
        for name, seen in ways:
            new = first_seen(night, seen)
            doms = {domain(h) for h in new}
            r_up = rank(new, PASTE[0], lambda h: (-new[h]["up"], h))
            r_abc = rank(new, PASTE[0], lambda h: h)
            rows.append((seed, name, len(new), len(doms), r_up, r_abc, new))
    for name, _ in ways:
        rs = [r for r in rows if r[1] == name]
        n = [r[2] for r in rs]
        dm = [r[3] for r in rs]
        ru = sorted({r[4] for r in rs})
        ra = [r[5] for r in rs]
        print("\n%s\n  first-seen hosts: mean %.1f (min %d, max %d); registrable domains: mean %.1f"
              % (name, sum(n) / len(n), min(n), max(n), sum(dm) / len(dm)))
        print("  paste.rs ranks %s by bytes sent, %d to %d of the list alphabetically"
              % ("/".join(str(x) for x in ru), min(ra), max(ra)))
        new = rs[0][6]
        top = sorted(new, key=lambda h: (-new[h]["up"], h))
        print("  seed %d, by bytes sent: %s" % (rs[0][0], ", ".join(
            "%s (%s, %s)" % (h, fmt(new[h]["up"]), new[h]["who"]) for h in top[:6])) + (" ..." if len(top) > 6 else ""))
    # The second night: the same work, other links followed, with the first night's hosts seen.
    seen2 = setup_hosts | {e["host"] for e in night_events(d, seeds[0])}
    n2 = [len(first_seen([e for e in night_events(d, s) if e["host"] != PASTE[0]], seen2)) for s in seeds[1:]]
    if n2:
        print("\nA second night of the same work, other links followed, the first night's hosts seen (way A): "
              "mean %.1f first-seen hosts (min %d, max %d)" % (sum(n2) / len(n2), min(n2), max(n2)))
    # Which seen hosts can carry data out: a host anyone can write to is no safer for having been seen.
    writable = {"github.com", "api.github.com", "storage.googleapis.com", "ghcr.io"}
    print("\nSeen after B or C, and writable by anyone with an account: %s"
          % ", ".join(sorted(h for h in (setup_hosts | programs) if h in writable)))


def ratios():
    """Bytes sent over bytes received, per host, in every measured log (the setup and a docs walk)."""
    per = {}
    for name in sorted(os.listdir(os.path.join(HERE, "data"))):
        for line in open(os.path.join(HERE, "data", name)):
            r = json.loads(line)
            a = per.setdefault(r["host"], [0, 0])
            a[0] += r["up"]
            a[1] += r["down"]
    out = sorted((u / max(d, 1), h, u, d) for h, (u, d) in per.items())
    print("\nMeasured hosts: %d. The one that sent most against what it received: %s, %s sent, %s received (%.2f)."
          % (len(out), out[-1][1], fmt(out[-1][2]), fmt(out[-1][3]), out[-1][0]))
    print("The paste upload of the scenario: %s sent, %s received (%.0f)." % (fmt(PASTE[1]), fmt(PASTE[2]), PASTE[1] / PASTE[2]))


def fmt(n):
    return "%.1f MB" % (n / 1e6) if n >= 1e6 else "%.0f kB" % (n / 1e3) if n >= 1e3 else "%d B" % n


if __name__ == "__main__":
    d = sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "..", "2026-11-24-gap-fixes", "prototypes", "holds", "data")
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 20
    main(d, list(range(n)))
    ratios()

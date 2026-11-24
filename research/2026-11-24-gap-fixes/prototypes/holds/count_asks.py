"""Count the asks a computer limited to a list of hosts would raise if a host not on the list were
held with an ask, replaying hosts really contacted.

Inputs (made by host_log_proxy.py and docs_hosts.py):
  npm.jsonl   CONNECTs of `npm install` of a 21-dependency web app, then `npx playwright install chromium`
  pip.jsonl   CONNECTs of `pip install -r requirements.txt` (20 packages), a spaCy model, NLTK data,
              `playwright install firefox`
  docs.json   20 real documentation pages for one task, with the hosts of their links

Ways of grouping, for each yes:
  host        one ask per new host name (port 443 for all of them here)
  domain      one ask per new registrable domain (Public Suffix List, private section included, so
              that evil.github.io and docs.github.io stay apart)
  burst       one ask per burst: new hosts first asked by the same command within WINDOW seconds of
              the first one join the open ask, which lists them all
Starting lists:
  day-one     api.anthropic.com, registry.npmjs.org, pypi.org, files.pythonhosted.org, github.com
  +downloads  day-one plus a shipped set of hosts that package install scripts download from
  +docs       +downloads plus a shipped set of documentation sites

Run: PYTHONPATH=<dir with publicsuffixlist> python3 count_asks.py <npm.jsonl> <pip.jsonl> <docs.json>
"""
import json, random, sys
from publicsuffixlist import PublicSuffixList

PSL = PublicSuffixList()
WINDOW = 60.0

DAY_ONE = {"api.anthropic.com", "registry.npmjs.org", "pypi.org", "files.pythonhosted.org", "github.com"}
DOWNLOADS = {  # hosts install scripts fetch binaries from; each with its worst case (see the doc)
    "release-assets.githubusercontent.com", "objects.githubusercontent.com", "raw.githubusercontent.com",
    "binaries.prisma.sh", "download.cypress.io", "cdn.cypress.io", "cdn.playwright.dev",
    "playwright.download.prss.microsoft.com", "storage.googleapis.com", "nodejs.org",
}
DOCS = {  # chosen as "well-known documentation sites", before looking at the walk below
    "developer.mozilla.org", "docs.python.org", "nodejs.org", "react.dev", "nextjs.org", "vercel.com",
    "docs.github.com", "www.typescriptlang.org", "tailwindcss.com", "www.prisma.io", "www.postgresql.org",
    "stackoverflow.com", "docs.stripe.com", "developer.apple.com", "learn.microsoft.com", "docs.aws.amazon.com",
    "cloud.google.com", "kubernetes.io", "docs.docker.com", "go.dev", "pkg.go.dev", "doc.rust-lang.org",
    "docs.rs", "www.npmjs.com", "en.wikipedia.org", "vitejs.dev", "vite.dev", "svelte.dev", "vuejs.org",
    "angular.dev", "docs.djangoproject.com", "fastapi.tiangolo.com", "flask.palletsprojects.com",
    "pandas.pydata.org", "numpy.org", "docs.pytest.org", "sqlite.org", "www.sqlite.org", "redis.io",
    "www.mongodb.com", "docs.astral.sh", "bun.com", "bun.sh", "deno.com", "docs.deno.com",
}
SOCIAL = {"x.com", "twitter.com", "bsky.app", "linkedin.com", "www.instagram.com", "www.facebook.com",
          "youtube.com", "mastodon.social", "fosstodon.org", "discord.gg"}


def domain(h):
    return PSL.privatesuffix(h) or h


def load_conn(path):
    """(time, host, command) for each CONNECT; the command is the process's parent, a report."""
    out = []
    for line in open(path):
        r = json.loads(line)
        p = r.get("proc") or {}
        out.append((r["t"], r["host"], (p.get("parent") or p.get("cmd") or "?")[:60]))
    return out


def docs_walk(path, seed, follow=2):
    """An agent reads the 20 pages, one every 20 s, and follows `follow` content links from each."""
    rnd = random.Random(seed)
    pages = json.load(open(path))
    t, out = 0.0, []
    for p in pages:
        host = p.get("final_host") or p["url"].split("/")[2]
        out.append((t, host, "webfetch")); t += 20
        links = sorted(h for h in p.get("link_hosts", {}) if h != host and h not in SOCIAL)
        for h in rnd.sample(links, min(follow, len(links))):
            out.append((t, h, "webfetch")); t += 20
    return out


def count(events, allowed, key):
    """Number of asks, and the hosts that raised one. A yes adds what the ask was about."""
    allowed_hosts = set(allowed)
    allowed_keys = {key(h) for h in allowed} if key is domain else set()
    asks, hosts = 0, []
    for _, h, _ in events:
        if h in allowed_hosts or (key is domain and domain(h) in allowed_keys):
            continue
        asks += 1; hosts.append(h)
        allowed_hosts.add(h)
        if key is domain:
            allowed_keys.add(domain(h))
    return asks, hosts


def count_burst(events, allowed):
    allowed_hosts = set(allowed)
    asks, open_ask = 0, {}
    for t, h, cmd in events:
        if h in allowed_hosts:
            continue
        o = open_ask.get(cmd)
        if o is None or t - o > WINDOW:
            asks += 1; open_ask[cmd] = t
        allowed_hosts.add(h)
    return asks


def report(name, events):
    distinct = sorted({h for _, h, _ in events})
    print(f"\n## {name}: {len(events)} connections, {len(distinct)} distinct hosts, "
          f"{len({domain(h) for h in distinct})} registrable domains")
    for label, base in (("day-one", DAY_ONE), ("+downloads", DAY_ONE | DOWNLOADS), ("+docs", DAY_ONE | DOWNLOADS | DOCS)):
        a_host, hs = count(events, base, None)
        a_dom, _ = count(events, base, domain)
        a_burst = count_burst(events, base)
        print(f"  {label:<11} per host {a_host:>3}   per domain {a_dom:>3}   per burst {a_burst:>3}   asked: {', '.join(hs[:14])}{' ...' if len(hs) > 14 else ''}")


if __name__ == "__main__":
    npm, pip_, docs = sys.argv[1:4]
    report("npm install of a web app, then playwright install", load_conn(npm))
    report("pip install of 20 packages, then model and browser downloads", load_conn(pip_))
    walks = [docs_walk(docs, s) for s in range(20)]
    report("docs walk, seed 0 (20 pages, 2 links followed from each)", walks[0])
    for label, base in (("day-one", DAY_ONE), ("+docs", DAY_ONE | DOWNLOADS | DOCS)):
        hs = [count(w, base, None)[0] for w in walks]
        ds = [count(w, base, domain)[0] for w in walks]
        print(f"  20 seeds, {label}: per host mean {sum(hs)/20:.1f} (min {min(hs)}, max {max(hs)}); "
              f"per domain mean {sum(ds)/20:.1f} (min {min(ds)}, max {max(ds)})")
    nolink = [docs_walk(docs, 0, follow=0)]
    print(f"  pages only, no link followed: per host {count(nolink[0], DAY_ONE, None)[0]}, "
          f"with +docs {count(nolink[0], DAY_ONE | DOWNLOADS | DOCS, None)[0]}")

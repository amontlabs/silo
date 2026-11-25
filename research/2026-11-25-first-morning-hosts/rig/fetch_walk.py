"""Fetches the pages of #350's documentation walk for one seed through HTTPS_PROXY (rig/hostlog.py),
so the log holds the bytes each way of each host an agent reading documentation reaches. GET only.

  python3 rig/fetch_walk.py DOCS_JSON SEED
"""
import json, random, sys, urllib.request

SOCIAL = {"x.com", "twitter.com", "bsky.app", "linkedin.com", "www.instagram.com", "www.facebook.com",
          "youtube.com", "mastodon.social", "fosstodon.org", "discord.gg"}
pages = json.load(open(sys.argv[1]))
rnd = random.Random(int(sys.argv[2]))
for p in pages:
    host = p.get("final_host") or p["url"].split("/")[2]
    urls = [p["url"]]
    links = sorted(h for h in p.get("link_hosts", {}) if h != host and h not in SOCIAL)
    urls += ["https://%s/" % h for h in rnd.sample(links, min(2, len(links)))]
    for u in urls:
        try:
            req = urllib.request.Request(u, headers={"user-agent": "Mozilla/5.0 (first-morning survey)"})
            with urllib.request.urlopen(req, timeout=20) as r:
                n = len(r.read(5_000_000))
            print("%s %d" % (u, n))
        except Exception as e:  # noqa: BLE001
            print("%s error %s" % (u, str(e)[:60]))

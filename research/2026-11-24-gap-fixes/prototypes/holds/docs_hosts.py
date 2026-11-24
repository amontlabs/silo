"""Fetch real documentation pages and list the hosts their links point to.

It stands in for an agent reading documentation for one task: "add Stripe Checkout to a Next.js
app that uses Prisma, and deploy it on Vercel". Each start page is fetched once (as WebFetch or
curl from inside the computer would), and the hosts of its <a href> links are kept, since those
are what the agent can follow next.

Run: python3 docs_hosts.py > docs_hosts.json    (uses HTTPS_PROXY and the system CA settings)
"""
import json, re, sys, urllib.parse, urllib.request

START = [
    "https://nextjs.org/docs/app/getting-started/installation",
    "https://nextjs.org/docs/app/api-reference/file-conventions/route",
    "https://react.dev/reference/react/useActionState",
    "https://docs.stripe.com/checkout/quickstart",
    "https://docs.stripe.com/webhooks",
    "https://docs.stripe.com/api/checkout/sessions/create",
    "https://www.prisma.io/docs/getting-started/quickstart-sqlite",
    "https://www.prisma.io/docs/orm/prisma-schema/data-model/models",
    "https://vercel.com/docs/environment-variables",
    "https://vercel.com/docs/functions",
    "https://developer.mozilla.org/en-US/docs/Web/API/Fetch_API/Using_Fetch",
    "https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Security-Policy",
    "https://www.typescriptlang.org/docs/handbook/2/narrowing.html",
    "https://tailwindcss.com/docs/installation/using-postcss",
    "https://nodejs.org/api/crypto.html",
    "https://docs.github.com/en/actions/writing-workflows/quickstart",
    "https://github.com/stripe/stripe-node",
    "https://www.npmjs.com/package/stripe",
    "https://stackoverflow.com/questions/76389957/nextjs-13-api-route-with-stripe-webhook",
    "https://www.postgresql.org/docs/current/sql-createindex.html",
]


def fetch(url):
    req = urllib.request.Request(url, headers={"user-agent": "Mozilla/5.0 (docs host survey)"})
    with urllib.request.urlopen(req, timeout=30) as r:
        return r.geturl(), r.read(3_000_000).decode("utf-8", errors="replace")


out = []
for u in START:
    rec = {"url": u}
    try:
        final, html = fetch(u)
        rec["final_host"] = urllib.parse.urlparse(final).hostname
        hosts = {}
        for href in re.findall(r'<a\s[^>]*href="([^"#]+)"', html, flags=re.I):
            h = urllib.parse.urlparse(urllib.parse.urljoin(final, href)).hostname
            if h:
                hosts[h] = hosts.get(h, 0) + 1
        rec["link_hosts"] = hosts
    except Exception as e:
        rec["error"] = str(e)[:200]
    out.append(rec)
    print(u, rec.get("final_host"), len(rec.get("link_hosts", {})), rec.get("error", ""), file=sys.stderr)
json.dump(out, sys.stdout, indent=1)

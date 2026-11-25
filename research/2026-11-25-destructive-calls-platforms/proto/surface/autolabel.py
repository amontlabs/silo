"""First pass of labels, by words in the operation's name and summary. Only a draft: every
operation was then read and the draft corrected in labels/<provider>.json (see label.md)."""
import json, re, sys, os
HERE = os.path.dirname(os.path.abspath(__file__))

def words(s):
    s = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", s or "")
    return s.lower()

GONE = r"\b(delete|deletes|deleting|remove|removes|destroy|purge|drop|truncate|wipe|erase|clear|empty|revert|overwrite|replace|reset|rollback|roll back|restore|unset|unpublish|detach|disconnect|release|unlink|uninstall)\b"
OUTAGE = r"\b(stop|pause|suspend|disable|kill|signal|cordon|sleep|rotate|revoke|cancel|scale|transfer|move|promote|nameservers|takedown|archive|leave|block)\b"
MONEY = r"\b(purchase|buy|billing|plan|subscription|usage limit|spend|invoice|payment|renew)\b"
EXPOSED = r"\b(invite|invitation|member|token|public|visibility|share|access|permission|role|password|credential|key)\b"
DEPLOY = r"\b(deploy|redeploy|deployment|release|build|upload|publish|version)\b"

def guess(op):
    w = words(op["name"] + " " + op.get("summary", ""))
    if op["method"] == "DELETE":
        return "hold", "gone"
    for rx, out in ((GONE, "gone"), (OUTAGE, "outage"), (MONEY, "money")):
        if re.search(rx, w):
            return "hold", out
    if re.search(DEPLOY, w):
        return "deploy", None
    if re.search(EXPOSED, w):
        return "check", "exposed"
    if op["method"] in ("PUT", "PATCH") or re.search(r"\b(update|edit|set|change|modify|patch|upsert)\b", w):
        return "check", None
    return "allow", None

if __name__ == "__main__":
    for prov in sys.argv[1:]:
        ops = json.load(open(os.path.join(HERE, "ops", prov + ".json")))
        for o in ops:
            l, out = guess(o)
            print("%-6s %-9s %-6s %-60s %s | %s" % (l, out or "", o["method"], o["path"][-60:], o["name"], o.get("summary", "")[:90]))

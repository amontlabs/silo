"""Runs the rule over a capture and prints, per case, what each way of tying uploads decides.

    python3 evaluate.py capture.jsonl

The computer's accounts: GitHub `polarzero`, the bucket `polarzero-builds`, the Google identity
polarzero@example.com. `mallory` is someone else. Each case is labelled with where its data really
ends up, and the table shows whether each way lets it through.
"""

import collections
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
sys.path.insert(0, str(Path(__file__).parent.parent / "capture"))
from uploads import Accounts, decide  # noqa: E402
from proxy import TOKENS  # noqa: E402  (what the host answers to "whose token is this")

# Where the data lands, for each case: "yours", "theirs" (someone else can read it), or "read".
TRUTH = {
    "git-push-own": "yours",
    "git-push-foreign-repo": "theirs",
    "git-push-foreign-repo-own-token": "theirs",
    "git-push-own-repo-foreign-token": "theirs",   # pushed as mallory; if GitHub takes it, mallory can
    "git-push-gist-foreign": "theirs",
    "git-fetch-own": "read",
    "git-clone-public-other": "read",
    "gh-gist-create-foreign": "theirs",
    "gh-gist-create-own": "yours",
    "gh-issue-own-repo-foreign-token": "theirs",   # a public issue in your repository, written as mallory
    "gh-issue-foreign-repo": "theirs",
    "gh-comment-foreign-repo-own-token": "theirs",
    "gh-release-upload-own": "yours",
    "gh-release-upload-foreign": "theirs",
    "gh-api-read": "read",
    "gh-graphql-read": "read",
    "docker-push-own": "yours",
    "docker-push-foreign": "theirs",
    "docker-push-own-name-foreign-token": "theirs",
    "gcs-gcloud-own": "yours",
    "gcs-gcloud-foreign": "theirs",
    "gcs-gcloud-foreign-resumable": "theirs",
    "gcs-gcloud-own-bucket-foreign-token": "theirs",
    "gcs-gcloud-read-own": "read",
    "gcs-gsutil-anonymous": "theirs",
    "gcs-signed-url-foreign": "theirs",
    "gcs-xml-virtual-host-anonymous": "theirs",
    "gcs-json-media-anonymous": "theirs",
    "gcs-alias-host-foreign": "theirs",
}

ACCOUNTS = Accounts(github=["polarzero"], buckets=["polarzero-builds"],
                    identities=["polarzero@example.com"])


def whose(form, value, host):
    """The host's answer about a credential: GitHub's GET /user, Google's tokeninfo.

    A signed URL names its signer in the URL itself; an API key names a project, not a person."""
    if form == "signed-url":
        return value.replace("%40", "@")
    if form == "api-key":
        return None
    return TOKENS.get(value)


def node_owner(node_id):
    """GitHub's answer to nodes(ids:): the owner of the repository a node belongs to.
    The fake's node ids carry it (R_<owner>_<repo>, I_<owner>_<repo>_<n>)."""
    parts = node_id.split("_")
    return parts[1] if len(parts) >= 3 else None


ORDER = {"allow": 0, "hold": 1, "refuse": 2}


def worst(ds):
    return max(ds, key=ORDER.get) if ds else "allow"


def main(path):
    recs = [json.loads(line) for line in open(path)]
    recs = [r for r in recs if r["case"] in TRUTH]
    per = collections.defaultdict(lambda: {"place": [], "both": [], "uploads": [], "up": 0, "down": 0,
                                           "hosts": set()})
    for rec, kind, place, form, who, by_place, by_both in decide(recs, ACCOUNTS, whose, node_owner):
        c = per[rec["case"]]
        c["place"].append(by_place)
        c["both"].append(by_both)
        c["up"] += rec["bytes_up"]
        c["down"] += rec["bytes_down"]
        c["hosts"].add(rec["host"])
        if kind != "read":
            c["uploads"].append(f'{rec["method"]} {rec["host"]} -> {place} [{form}{"=" + who if who else ""}]')

    def stops(d):
        return d in ("refuse", "hold")

    score = collections.Counter()
    print(f'{"case":38} {"truth":6} {"sent/back":>10} {"unread":8} {"place":7} {"place+cred":10}')
    for case in TRUTH:
        if case not in per:
            print(f"{case:38} {TRUTH[case]:6}  (no request captured)")
            continue
        c = per[case]
        ratio = c["up"] / max(c["down"], 1)
        unread = "flag" if ratio > 1 else "-"
        p, b = worst(c["place"]), worst(c["both"])
        print(f'{case:38} {TRUTH[case]:6} {ratio:10.2f} {unread:8} {p:7} {b:10}')
        for u in dict.fromkeys(c["uploads"]):
            print(f"{'':42}{u}")
        t = TRUTH[case]
        for way, d in (("unread", unread == "flag"), ("place", stops(p)), ("place+cred", stops(b))):
            if t == "theirs":
                score[(way, "theirs stopped")] += d
                score[(way, "theirs")] += 1
            else:
                score[(way, f"{t} stopped")] += d
                score[(way, t)] += 1
    print()
    for way in ("unread", "place", "place+cred"):
        print(f'{way:11} stops {score[(way, "theirs stopped")]} of {score[(way, "theirs")]} uploads to someone else; '
              f'stops {score[(way, "yours stopped")]} of {score[(way, "yours")]} of yours; '
              f'stops {score[(way, "read stopped")]} of {score[(way, "read")]} reads')


if __name__ == "__main__":
    main(sys.argv[1])

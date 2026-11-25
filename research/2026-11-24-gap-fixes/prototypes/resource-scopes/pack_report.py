"""Prints, for each push in a corpus, what the boundary can read from its pack alone."""
import base64
import json
import sys

from classify import commit_fields, pkt_lines, read_pack, workflow_trees

for line in open(sys.argv[1]):
    r = json.loads(line)
    if r["method"] != "POST" or not r["target"].endswith("git-receive-pack"):
        continue
    b = base64.b64decode(r["body_b64"])
    pos = 0
    for pkt, pos in pkt_lines(b):
        if pkt is None:
            break
    objs, thin = read_pack(b[pos:])
    commits = [s for s, (t, _) in objs.items() if t == "commit"]
    roots = sum(commit_fields(objs[c][1])[0] in objs for c in commits)
    wf = [workflow_trees(objs, commit_fields(objs[c][1])[0]) for c in commits]
    print(f"{sys.argv[2]:8} pack {len(b) - pos} bytes, {len(objs)} objects, {len(thin)} deltas on objects "
          f"outside the pack, root trees readable {roots}/{len(commits)}, .github ids read {sum(1 for w in wf if w)}")

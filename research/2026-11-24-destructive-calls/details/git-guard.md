# git push at the boundary: a measured prototype

Question: when an agent runs `git push` over HTTPS to a host whose traffic Silo reads, can Silo tell a branch delete or a force push from a normal push, keep the old commit before letting it pass, and refuse in words the agent sees? How much time does that add?

Everything here ran on this Linux container on 2026-11-24, with git 2.43.0 and Python 3.13. Nothing was pushed to GitHub or any other remote. No token was used. Web sources were read on 2026-11-24.

Code: `../proto/git-guard/`. Raw results: `../proto/git-guard/results/runs.json` (290 pushes), `summary.md` (tables made by `analyze.py`), `bench.json` (scale check).

## Findings

1. **Delete and create are visible in the request. Force is not.** A delete has forty zeros as the new id. A create has zeros as the old id. A force push and a fast-forward look the same: two ids and a ref. Telling them apart needs the commit graph. All 281 ref updates in 290 real pushes were classified correctly (15 scenarios, 6 modes, 5 repetitions of the timed modes).
2. **Neither the pack alone nor the server alone is enough. Together they always were.**
   - The pack alone proved every fast-forward: walking parents from the new commit through the commits in the pack reached the old commit. It never proved a force push. The walk stops at a commit the server already has, and the pack cannot say where that commit stands.
   - The server alone (`is old an ancestor of new?`) failed for every push that carried new commits, because the new commit is not on the server until the push lands. It only worked when the agent rewound a branch to a commit the server already had (`git push -f origin HEAD~2:main`). On GitHub this means `compare/{old}...{new}` cannot answer before the push for most pushes (inferred, not tested against GitHub).
   - Together: walk the pack; if it does not reach the old commit, ask the server whether the old commit is an ancestor of each commit where the walk left the pack. One question was enough in every case here. On GitHub the question is `GET /repos/{o}/{r}/compare/{old}...{boundary}`, which needs only Contents read.
3. **The cost is small next to a push.** Parse plus classification plus backup took 0.1 to 5 ms per push for small pushes, and the push itself took about 140 ms end to end on loopback. A 1.6 MB push added about 60 ms, almost all of it reading and hashing the body in Python. A synthetic 8.8 MB push of 10,000 commits took 509 ms to read in full, but only 77 ms when reading stops at the first tree or blob. git writes all commits first in a pack, so Silo needs only the first part of the pack to decide.
4. **A backup ref makes a delete or force push undoable, and the agent must not be able to touch it.** Before passing, the guard wrote `refs/silo/backup/<ref>/<timestamp>` at the old commit. The old commit survived `git gc --prune=now` in every delete and force case. But `git push --mirror` deletes every remote ref the agent's clone lacks, and backup refs are such refs. On the local server, `transfer.hideRefs=refs/silo` hid them from clones and made git itself refuse to change them. GitHub has no such setting, and it does advertise refs outside heads and tags (`refs/pull/*`, checked today). So Silo must refuse any push, REST call or GraphQL mutation that touches `refs/silo/` itself.
5. **The refusal git shows best is a 200 receive-pack answer with `ng <ref> <reason>`.** With side-band framing, git printed the sentence twice: as `remote: Held by Silo: ...` and as `! [remote rejected] main -> main (Held by Silo: ...)`, and exited 1. Hold then pass delivered the same bytes the client sent (sha256 equal on 165 of 165 passed pushes, including the large chunked push). Silo can also split a push: refuse the force push of `main` and pass the fast-forward of `feature` in the same push. git showed one ref updated and one rejected. An atomic or signed push cannot be split; the guard refuses all of it.

## How the prototype works

```
git (agent) --HTTPS 127.0.0.1:844x, test CA via GIT_SSL_CAINFO--> guard.py --HTTP--> backend.py --CGI--> git http-backend --> bare repo
```

- `receivepack.py`: reads pkt-lines, capabilities after the NUL, `shallow` lines, push options (only when the client sent the `push-options` capability), a signed push's certificate, the probe body `0000`, and the pack. It reads pack objects with zlib, resolves deltas inside the pack, and computes commit ids. It builds refusals and splices refusals into the server's own answer.
- `guard.py`: one process per mode. `pass` reads nothing (baseline). `observe` runs all three ancestry methods and passes. `backup` creates the backup ref, then passes. `refuse` answers `ng`. `hold` waits 2 s (standing in for the ask, answered yes), backs up, then passes. `split` refuses only the destructive refs. Every mode except `pass` refuses changes under `refs/silo/`.
- `backend.py`: plain HTTP in front of `git http-backend`. It logs the sha256 of every body it receives, which is how byte-for-byte delivery is checked.
- `scenarios.py`: builds a fresh bare repo and clone per run, pushes with real git, records exit code, what git printed, which server refs changed, and both logs. In `backup` and `hold` it then runs `git gc --prune=now` on the server and checks the old commit is still there.
- `bench.py` and `packorder.py`: the scale check and the order of objects in a push pack.

The client environment had no proxy variables, no tokens and its own `HOME`. `/etc/hosts` was not touched.

## Classification

Wire format (source: https://git-scm.com/docs/gitprotocol-pack): `create = zero-id SP new-id SP name`, `delete = old-id SP zero-id SP name`, `update = old-id SP new-id SP name`. "The packfile MUST NOT be sent if the only command used is _delete_." "A packfile MUST be sent if either create or update command is used, even if the server already has all the necessary objects."

| Scenario (real git command) | Ref | Expected | Got | Pack alone | Server alone | Server questions |
|---|---|---|---|---|---|---|
| normal: `git push origin main` | main | ff | ff | ff | n/a (new not on server) | 0 |
| new branch: `git push origin topic` | topic | create | create | | | |
| branch delete: `git push origin --delete feature` | feature | delete | delete | | | |
| force push: reset, new commit, `git push --force origin main` | main | force | force | unknown | n/a | 1 |
| rewind: `git push --force origin HEAD~2:main` | main | force | force | unknown (empty pack) | force | 1 |
| rebase: feature reset onto main, `git push --force origin feature` | feature | force | force | unknown | n/a | 1 |
| merge commit: `git push origin main` | main | ff | ff | ff | n/a | 0 |
| several refs: `git push --force origin main feature` | main / feature | force / ff | force / ff | unknown / ff | n/a | 1 / 0 |
| atomic: `git push --atomic --force origin main feature` | main / feature | force / ff | force / ff | unknown / ff | n/a | 1 / 0 |
| mirror: `git push --mirror origin` | main, feature, old-feature, v1 | ff, delete, delete, delete | same | | | |
| tag delete: `git push origin --delete refs/tags/v1` | v1 | delete | delete | | | |
| push options: `git push -o ci.skip -o merge_request.create origin main` | main | ff | ff | ff | n/a | 0 |
| large (3 MB file, probe first) | main | ff | ff | ff | n/a | 0 |
| large force push (probe first) | main | force | force | unknown | n/a | 1 |
| signed: `git push --signed --force origin main` | main | force | force | unknown | n/a | 1 |
| `git push origin main:refs/silo/backup/heads/main/forged` | refs/silo/... | create | refused (Silo's namespace) | | | |

Notes:

- **Mirror is the most destructive command here.** One `git push --mirror` from a clone with only `main` checked out deleted `feature`, `old-feature` and tag `v1`, and created four `refs/remotes/origin/*` refs on the server. The agent did not name any of those refs.
- **Moving a tag** (old and new both set under `refs/tags/`) is classed `tag-move` and treated like a force push. Not exercised by a scenario.
- **Probe.** For the 3 MB pushes git first sent a POST whose body was only `0000`, then the real request as a chunked body. The guard passed the probe and decided on the second POST. This matches `cli-capture.md`.
- **Signed push.** The commands sit inside a `push-cert` block, not in the usual list, and the capabilities follow the NUL on the `push-cert` line. A parser that only reads the usual list sees no commands at all. The guard reads the certificate. Anything it cannot read is refused, never passed.
- **Not handled:** protocol version 2 for push (git 2.43 pushed with the version 0 format in every run), SHA-256 repositories (64-hex ids), and a REF_DELTA commit whose base is only on the server. No commit was stored as a delta in any pack here (scenarios and the 11,530 commits of the scale check), but the pack format allows it.

## Ancestry: the three methods, timed

Inside `observe`, median per ref over 5 runs:

| Case | (a) pack walk | (b) server `merge-base --is-ancestor old new` | (c) pack walk, then server for the boundary |
|---|---|---|---|
| fast-forward, small | 0.1 ms, decides | 1.7 ms, cannot answer (new not on server) | 0.1 ms, no server question |
| force push, small | 0.1 ms, cannot decide | 1.7 ms, cannot answer | 1.9 ms, decides with 1 question |
| rewind to an existing commit | 0.0 ms, empty pack | 3.7 ms, decides | 1.8 ms, decides |
| fast-forward, 1.6 MB pack | 5.8 ms | 2.2 ms, cannot answer | 5.8 ms |

Scale check (`bench.py`, synthetic history built with `git fast-import`, the pack made the way `send-pack` makes it: `pack-objects --revs --thin --delta-base-offset`):

| Case | Pack | Read whole pack | Read until the first tree or blob | Walk | Server question |
|---|---|---|---|---|---|
| 10 new commits on 4,990 | 3 KB | 0.3 ms | 0.1 ms | 0.01 ms, ff | none |
| 1,000 new commits on 4,000 | 0.65 MB | 42 ms | 10 ms | 0.2 ms, ff | none |
| 10,000 new commits on 10,000 | 8.8 MB | 509 ms | 77 ms | 2.3 ms, ff | none |
| 20 commits rewritten, 5,000 on server | 7 KB | 0.4 ms | 0.1 ms | unknown | 2.4 ms, force |
| 500 commits rewritten, 20,000 on server | 0.36 MB | 18 ms | 2.8 ms | unknown | 2.6 ms with commit-graph, 4.0 ms without, force |

The commits-first reading gave the same commit graph as the full reading in all five cases. In a 1,000-commit push the last commit object ended at 25% of the pack (`packorder.py`). git's `pack-objects` writes objects in layers: "Then fill all the tagged tips.", "And then all remaining commits and tags.", "And then all the trees.", "Finally all the rest in really tight order" (comments in `compute_layer_order`, https://raw.githubusercontent.com/git/git/v2.43.0/builtin/pack-objects.c). So Silo can classify a push while most of the pack is still arriving. It must still keep the whole body to forward it after a hold.

Python is slow at this. A native implementation would be faster; these figures are an upper bound.

### The same question on GitHub

`GET /repos/{owner}/{repo}/compare/{basehead}` with `basehead` = `{old}...{boundary}`. The response has `status` (one of `diverged`, `ahead`, `behind`, `identical`), `ahead_by` and `behind_by`. Without paging the commit list is limited to 250 commits, and files to 300. Source: https://docs.github.com/en/rest/commits/commits#compare-two-commits . The page lists the values but does not define them. Read in git's terms: `ahead` or `identical` means the old commit is an ancestor of the boundary commit (fast-forward), `behind` or `diverged` means it is not (force). This reading is the usual one, not quoted from the page.

- Permission: fine-grained token, repository permission "Contents", read. Source: https://docs.github.com/en/rest/authentication/permissions-required-for-fine-grained-personal-access-tokens (row `GET /repos/{owner}/{repo}/compare/{basehead}` under "Contents"). Silo holds the token, so it can ask with it. Silo could ask even with a secret limited to "only read", since the question is a read.
- To keep the response small, ask with `?per_page=1`. The documented paging parameter makes this possible. That `status` is still exact with it is unverified.
- Not measured: GitHub's latency for this call. One call per force-push candidate. A fast-forward that the pack proves needs no call.

## Making a delete or force push undoable

### Local server (done for real)

`git update-ref refs/silo/backup/<ref without refs/>/<UTC timestamp> <old> 0000…` (the zero old value means "only if it does not exist yet"). It took about 2 ms per ref. After `git reflog expire --expire=now --all` and `git gc --prune=now`, the old commit was still in the server repository in every backup and hold run (deletes, force pushes, rewinds, tag delete, signed push, the three deletes of the hidden-refs mirror push). If the backup fails, the guard refuses the push instead of passing it.

Visibility without hidden refs: `git ls-remote` and `git clone --mirror` showed the backup refs. A normal `git clone` fetches only heads and tags, so it does not. The agent's `git push --mirror` then tried to delete the backup ref. The guard refused the whole push, because one of its refs was under `refs/silo/`. In split mode it could pass the rest.

With `transfer.hideRefs = refs/silo` on the server, the backup refs were not advertised, the mirror push did not try to delete them, and git itself refused a direct attempt: `! [remote rejected] refs/silo/backup/x (deny deleting a hidden ref)` and `(deny updating a hidden ref)` (tested with git 2.43.0; setting documented at https://git-scm.com/docs/git-config, entry `transfer.hideRefs`).

### GitHub (from docs, not tested)

- `POST /repos/{owner}/{repo}/git/refs` with `{"ref": "refs/silo/backup/heads/main/<ts>", "sha": "<old>"}`. The docs say of `ref`: "If it doesn't start with 'refs' and have at least two slashes, it will be rejected." Answers 201, 409, 422. Source: https://docs.github.com/en/rest/git/refs#create-a-reference . `refs/silo/backup/...` meets that rule. **Whether GitHub accepts a namespace outside heads and tags is unverified**: no page says so either way. The list endpoint returns "Anything in the namespace … not just heads and tags", including notes and stashes (same page).
- Permission: "Contents" write, and the endpoint is also listed under "Workflows" write ("Multiple permissions are required, or a different permission may be used"). Source: the permissions page above. A token that can push already has Contents write. When Workflows write is needed for a ref (likely when the commit touches `.github/workflows`) is unverified.
- Hidden refs: GitHub offers no `hideRefs` setting to users (unverified as a negative). GitHub does advertise refs outside heads and tags: `git ls-remote https://github.com/octocat/Hello-World.git` today listed `HEAD`, 3 `refs/heads/*` and 4,103 `refs/pull/*`. So a backup ref there would most likely show in `ls-remote`, in `clone --mirror`, and as a delete in the agent's `push --mirror` (unverified). It would not show in a normal clone, which fetches heads and tags only. GitHub marks `refs/pull/` read-only, so a push that touches it fails (https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/removing-sensitive-data-from-a-repository). No such protection exists for a ref Silo creates.
- Therefore, on GitHub, Silo itself must refuse anything that changes `refs/silo/`: a receive-pack command, `PATCH` or `DELETE /repos/{o}/{r}/git/refs/silo/...`, and the GraphQL `deleteRef`, `updateRef` and `updateRefs` mutations on such a ref (names from `saas.md`). Silo also needs to clean the backups up later, with the same token.
- Whether a ruleset could protect `refs/silo/**` is unverified. Rulesets target branches and tags.

### Does GitHub keep the old commit anyway?

- GitHub's engineering blog (Taylor Blau, 2022-09-13): "Normally, we don't remove any unreachable data from repositories." It also says unreachable objects go in cruft packs with per-object times, and expire on a date basis. It gives no production grace period and no schedule. https://github.blog/engineering/architecture-optimization/scaling-gits-garbage-collection/
- GitHub docs on removing sensitive data: after a history rewrite and force push, commits "may still be accessible elsewhere", including "Directly via their SHA-1 hashes in cached views on GitHub", until GitHub Support runs garbage collection. https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/removing-sensitive-data-from-a-repository
- Truffle Security (third party, 2024) showed commits from deleted forks still readable by sha through the parent repository. GitHub called this intended. https://trufflesecurity.com/blog/anyone-can-access-deleted-and-private-repo-data-github
- The activity API lists `force_push` and `branch_deletion` with before and after shas (`saas.md`).
- So after a force push on GitHub the old commit usually survives for an unstated time, and its sha is findable. **How long is not documented.** Whether `git fetch origin <sha>` works for an unreachable commit is unverified. The web view by sha does work (per the docs above). Recovery then means finding the sha and creating a ref at it, the same call as the backup. A backup ref made before the push turns "usually, for an unknown time" into "until Silo deletes the ref".

## Refusing the way git shows it

Report-status grammar (https://git-scm.com/docs/gitprotocol-pack): `unpack ok`, then per ref `ok <ref>` or `ng <ref> <error-msg>`, then a flush. When the client asked for `side-band-64k` (git did in every run), the report travels in band 1 and a message in band 2 prints as `remote: ...`. HTTP status stays 200, content type `application/x-git-receive-pack-result`.

What the agent saw (refuse mode, force push):

```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.
To https://127.0.0.1:8446/o/force-main.git
 ! [remote rejected] main -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
error: failed to push some refs to 'https://127.0.0.1:8446/o/force-main.git'
```

Exit code 1. A branch delete shows `! [remote rejected] feature (Held by Silo: deletes refs/heads/feature. ...)`. When one ref of a push is held, the others get a sentence of their own: `(refused with the held refs of this push)`, or for `--atomic`, `(atomic push: refused with the held refs)`.

Split mode, force of `main` plus fast-forward of `feature` in one push:

```
remote: Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.
   07cb202..4b54de7  feature -> feature
 ! [remote rejected] main -> main (Held by Silo: rewrites refs/heads/main (not a fast-forward). Ask the user in the Silo app.)
```

To split, the guard rewrites the command list (keeping the capabilities on the first kept command), forwards the same pack, then adds `ng` lines into the server's own answer. The objects for the refused ref then sit unreferenced on the server. A signed push cannot be split: removing a command breaks the signature. An atomic push must not be split: the agent asked for all or nothing.

Hold then pass: in `hold` mode the guard kept the request 2 s, created the backup ref, and forwarded it. The server received the same bytes the client sent (sha256 compared on every passed push: 165 equal, 0 different). The chunked large push was forwarded with a `Content-Length` instead; the body bytes were identical. git waited without complaint. `cli-capture.md` found no git timeout within 120 s.

## Time added

Median end-to-end `git push` wall time over 5 runs, on loopback with TLS:

| Scenario | No reading (`pass`) | Classify (`observe`) | Classify + backup (`backup`) | Guard work in `backup` |
|---|---|---|---|---|
| normal | 138 ms | 146 ms | 142 ms | 0.2 ms |
| branch delete | 129 ms | 131 ms | 135 ms | 2.2 ms |
| force push | 139 ms | 140 ms | 140 ms | 4.1 ms |
| several refs (force + ff) | 148 ms | 151 ms | 145 ms | 4.5 ms |
| large 3 MB push | 489 ms | 401 ms | 403 ms | 64 ms |

Run-to-run noise (about ±10 ms) is larger than the guard work for small pushes. For the large push the guard work is reading and hashing 1.6 MB in Python. Against GitHub, add one compare call for each force-push candidate and one ref creation for each delete or force that passes. Those were not measured.

## What this means for Silo

- A rule can hold "delete a branch or tag" from the request alone. "Force push" is a fact Silo can know only by asking the host one question, with the token it already holds, for each ref the pack does not prove to be a fast-forward.
- Before a delete or force push passes, Silo can create one ref with the same token. Then the outcome of a git delete or force push on GitHub becomes **recoverable** for as long as Silo keeps the ref.
- Silo's own refs need a rule that no request from the computer can change them, on every way to change a ref (git push, REST, GraphQL).
- `git push --mirror` should be shown in an ask as the list of deletes it makes. It is the case where the agent destroys refs it never named.
- The refusal must be git's own answer. `cli-capture.md` showed that a 403 loses the sentence and git prints "Everything up-to-date".

## Unverified

- GitHub accepting `refs/silo/...` through `POST /git/refs` or through a push. GitHub advertising such a ref. A ruleset protecting it.
- GitHub `compare` latency, and its exact answer for a commit that is not yet on GitHub (expected 404). Whether `per_page=1` changes `status`.
- How long GitHub keeps unreachable commits. Whether they can be fetched by sha over git.
- When `POST /git/refs` needs Workflows write.
- Push over protocol v2, SHA-256 repositories, and REF_DELTA commits against server objects. None of these appeared.
- Other hosts (GitLab, Bitbucket, Gitea). The receive-pack format is git's own, so the parsing should carry over. Each host's ref API and hidden-ref behaviour differ and were not checked.

## Rerun

```
cd research/2026-11-24-destructive-calls/proto/git-guard
REPS=5 python3 scenarios.py <scratch dir>     # writes results/runs.json
python3 analyze.py > results/summary.md
python3 bench.py <scratch dir>                # writes results/bench.json
```

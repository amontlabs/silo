# Uploads to hosts already seen

An agent's research, 25 November 2026, for polarzero. It takes up the last finding of #406: once
github.com, api.github.com, ghcr.io and storage.googleapis.com are seen, anyone with an account can
upload to each of them, so a log sent to a gist raises no first-seen alert. This page measures rules
that tie such uploads to your own accounts, owners and buckets, on captures of real `git push`,
`docker push`, `gcloud storage cp`, `gsutil cp`, `gh release upload`, `gh gist create` and
`gh issue create`.

## What was found

1. **A rule can tell your upload from someone else's on these hosts, from the request alone.** Every
   upload names its place in its own bytes: the owner in a Git or REST path, the namespace in a
   GHCR path, the bucket in a GCS path or host. A GraphQL mutation names a node id, which GitHub
   resolves (#350's lookup). The rule reads all of it once the boundary ends the encrypted
   connection, as `ending-the-encrypted-connection` already does for hosts Silo reads.
2. **The place alone stops 15 of 19 uploads to someone else, and none of yours.** The four it lets
   through land in a place of yours under someone else's credential. Three of those the real host
   would refuse anyway, since the other account cannot write there. The fourth it would take: an
   issue written by another account in your public repository, `gh issue create -R polarzero/app`
   with that account's token, is published at once and readable by its writer.
3. **The place and the credential together stop 19 of 19, and none of yours.** The credential is
   yours when it is a Silo placeholder or when the host says it is yours: GitHub's `GET /user`,
   Google's `tokeninfo`. A GHCR push carries an opaque registry token, but the boundary sees the
   `/token` exchange that made it, so it knows whose credential stands behind it. A GCS signed URL
   names its signer in the URL.
4. **Not reading leaves only the bytes, and they cannot tell yours from theirs.** "Sent more than it
   got back" (#406's line of its own, applied to seen hosts) marks 17 of 19 uploads to someone else,
   but also all 5 of yours and 2 of 5 reads. The two it misses were pushes of objects the fake host
   already held, so they carried almost nothing.
5. **An upload needs no credential at all on GCS.** A bucket that lets anyone write takes
   `gsutil cp` with no sign-in (gsutil sends only its built-in API key, which names a Google Cloud
   project and no person), a plain `PUT` to `<bucket>.storage.googleapis.com`, and a signed URL
   made elsewhere. A rule on credentials alone, such as `foreign-credential-refused` in #376, misses
   all three. The rule on the place stops all three.
6. **Seven hosts carry these uploads, not four.** github.com, api.github.com, uploads.github.com
   (`gh release upload`), gist.github.com (a push to a gist), ghcr.io, storage.googleapis.com, and
   www.googleapis.com, which takes the same GCS upload path. Each bucket has a host of its own as
   well. A rule written for #406's four hosts misses three of them.
7. **Paths a host may read otherwise must be held, not placed.** The first version of the rule
   placed `/repos/polarzero%2F..%2Fmallory/drop/issues` in `polarzero`, followed a dot segment, ran
   a GET with `X-HTTP-Method-Override: POST` as a read, and read a GCS destination bucket
   `polarzero-builds%2F..%2Fmallory-drop` as yours. The tests showed all four failing
   ([`results-evasions-before-fix.txt`](results-evasions-before-fix.txt)); the rule now holds an
   encoded slash, a dot or a backslash in the part of the path that names the place, any
   method-override header, and a bucket name Google would not accept, and the tests pass.

| Way | Uploads to someone else stopped | Your uploads stopped | Reads stopped |
|---|---|---|---|
| Not read: sent more than it got back | 17 of 19 | 5 of 5 | 2 of 5 |
| Read: the place is in your accounts | 15 of 19 | 0 of 5 | 0 of 5 |
| Read: the place, and the credential is yours | **19 of 19** | **0 of 5** | **0 of 5** |

Recommended: the place and the credential, on the seven hosts above, once you have named your
accounts. A write on those hosts that the rule cannot place, such as a form post to github.com, is
held.

## What it changes in the plan

| Entry | Change |
|---|---|
| `uploads-to-your-places` (new, open) | The rule above, with the options and the case, in `boundary`, after `hosts-already-seen` |

It rests on two entries in other drafts, named in its notes and not linked, since neither is on
`main`: #406's notes on `new-host-alert-is-a-rule` (seen hosts take uploads), and #376's
`foreign-credential-refused`, which refuses a credential that is not Silo's on a host that gets a
secret. This rule is the same idea for hosts that get no secret, keyed on the place as well, so it
also stops the three uploads with no credential. Ruling one does not rule the other.

## For polarzero to rule

- **Read seven upload hosts even when they get no secret.** It challenges
  `ending-the-encrypted-connection`, "Every other connection passes encrypted and unread", on
  github.com, as #350 did. The new case: an anonymous `PUT` to a bucket that lets anyone write
  carries no credential, so no rule about credentials catches it, and only a reading boundary sees
  the bucket. Recommended.
- **Name your accounts once, for every computer.** The rule needs the list: GitHub accounts and
  organisations, Google identities, buckets. Silo can read most of it from the secrets you give
  (whose token, which buckets it reaches), but the list should be yours to confirm. Recommended:
  asked in the first run, beside the secrets.

## The research at a glance

On seen hosts, a rule that reads uploads and lets them through only to your own accounts under
your own credential stopped 19 of 19 uploads to someone else and none of yours, across real git,
docker, gcloud, gsutil and gh; counting bytes alone flagged every upload of yours too.

## How it was measured

| File | What it is |
|---|---|
| [`capture/proxy.py`](capture/proxy.py) | A recording proxy that ends TLS and answers as GitHub, GHCR and GCS, enough for the real clients to send their real uploads. Nothing leaves the machine. Started from #350's `record_proxy.py` |
| [`capture/run_captures.sh`](capture/run_captures.sh) | The 29 cases, run with git 2.43.0, gh 2.89.0, Docker 29.9.3, Google Cloud SDK 570.0.0 (`gcloud storage`, `gsutil`) and curl |
| [`capture/slim.py`](capture/slim.py), [`data/capture.jsonl`](data/capture.jsonl) | The capture, with long bodies cut and gsutil's built-in key masked; [`data/clients.log`](data/clients.log) is what the clients printed |
| [`rule/uploads.py`](rule/uploads.py) | The rule: what each request puts where, under whose credential, and the decision each way |
| [`rule/evaluate.py`](rule/evaluate.py), [`results.txt`](results.txt) | Runs the rule over the capture, case by case |
| [`rule/test_uploads.py`](rule/test_uploads.py) | 21 tests on hand-written requests, including the evasions above |

`bash capture/run_captures.sh OUT` (as root, for Docker), then `python3 rule/evaluate.py OUT/capture.jsonl`
and `cd rule && python3 -m unittest test_uploads.py`.

## What was not verified

- No real host was reached. Every answer is the fake's, written from each API's documentation and
  from what the clients accepted. GitHub's, GHCR's and Google's real behaviour on the edge cases
  (an encoded slash, a method override) was not tried; the rule holds them either way.
- "Whose credential" is played from a table. On the real hosts it is one read-only request per new
  credential (`GET /user`, `tokeninfo`), sent by the boundary with that credential. Its cost and
  how long an answer is kept were not measured. A GitHub App's installation token answers
  `GET /user` with an error; such tokens would need `GET /installation/repositories` instead.
- A few clients failed after their upload was sent, on a detail of the fake (gcloud deletes an
  object whose hash it doubts; some `gh` commands want more GraphQL fields). The upload request is
  in the capture in every case; `data/clients.log` shows each exit.
- The ghcr.io blob data goes to the same host here. On the real GHCR, a blob upload may be sent on
  to a storage host; if so, that host needs reading too.
- Other ways out through GitHub were not captured: an image attached to an issue (uploaded to an
  S3 host of GitHub's), Actions artifacts, Codespaces, the GitHub CLI's `gh attestation`. S3 and
  AWS signing are #350's open question.
- Bytes are counted from the HTTP requests and answers the proxy saw, not from the connection, so
  TLS overhead is left out.
- A public issue or comment in your own repository, written with your own credential, passes. It
  is your upload by every test here. Whether publishing from a computer should be held is #350's
  "expose" effect, a separate entry.

## Sources

- #406, `research/2026-11-25-first-morning-hosts/` (seen hosts take uploads).
- #350, `research/2026-11-24-gap-fixes/2-secret-resource-scopes.md` (places on a host, node-id
  lookups, "a credential that is not Silo's") and its `record_proxy.py`.
- #376, `plan/boundary/decisions/foreign-credential-refused.json`.
- Docker Registry HTTP API V2 and its token authentication; Google Cloud Storage JSON and XML APIs,
  uploads and V4 signed URLs; GitHub REST (releases, gists, issues) and GraphQL.

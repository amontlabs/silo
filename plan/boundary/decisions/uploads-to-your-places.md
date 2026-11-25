# Where may a computer upload on a host it has already reached

## The choice

An agent's part, 25 November 2026, from `research/2026-11-25-uploads-to-seen-hosts/`.

The case: main has cloned from github.com and pulled an image from ghcr.io, so both hosts are seen
and raise no alert. A package install runs a script that reads `~/.aws` and `.env`, then sends
them out in four ways: a push to `mallory/drop` on github.com with a token of its own, a secret
gist, an issue in your own public repository `polarzero/app` written with that token, and a `PUT`
with no sign-in to a Google Cloud Storage bucket that lets anyone write. The record shows bytes sent
to hosts you use every day.

The options:

- Your places, under your credential (recommended). The boundary reads each request that writes on
  seven upload hosts: github.com, api.github.com, uploads.github.com, gist.github.com, ghcr.io,
  storage.googleapis.com, www.googleapis.com, and each bucket's own host. An upload passes only to a
  place in the accounts you named, and only under a Silo placeholder or a credential the host says
  is yours. All four sends above are refused. Measured on real `git`, `gh`, `docker`, `gcloud` and
  `gsutil` uploads: 19 of 19 uploads to someone else stopped, none of your 5. It costs reading those
  hosts, each a place where a program can refuse Silo's certificate; a write Silo cannot place is
  held; and you name your accounts once.
- Your places, whatever the credential. The same reading, without asking the host whose credential
  it is. It stops 15 of 19, and lets through the issue in your public repository, which is
  published at once.
- Not read. The record keeps bytes each way per host, and a seen host that was sent more than it
  sent back gets a line, as #406 proposes for first-seen hosts. It flagged 17 of 19 uploads to
  someone else, all 5 of yours and 2 of 5 reads, so the line comes every time you push.

Recommended: your places, under your credential. A seen host is where an upload hides best, and
the place is written in every upload's own bytes.

## Notes

This rule is #376's `foreign-credential-refused` carried to hosts that get no secret, and keyed on
the place too. The place is what stops an upload that carries no credential at all: `gsutil cp`
with no sign-in, a plain `PUT` to a bucket's host, and a signed URL made elsewhere were each
refused by the place and carried nothing a credential rule could refuse.

It challenges `ending-the-encrypted-connection` ("Every other connection passes encrypted and
unread") for these seven hosts, as #350 did for github.com.

Where you name your accounts is open. Silo can propose the list from the secrets you give, by
asking each host whose credential it is and which buckets it reaches, for you to confirm in the
first run.

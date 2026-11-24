# How does a computer use a secret it cannot read

## The choice

An agent's part, 22 November 2026. An agent in main uses your OpenAI key, which is sent as it is in
each request. It also uses your AWS keys, which are never sent: the program signs each request with
them inside the computer.

The options:

- A placeholder, swapped on the way out (recommended). The computer holds a stand-in, and Silo puts
  the real value in only on the way to the hosts you allowed. It costs the kinds of secret that are
  not sent as they are: AWS keys, SSH keys and database passwords stay ordinary files the agent can
  read, until a way is found for each. Silo also opens the encrypted traffic to those hosts.
- The real value inside, as everywhere else. The computer gets the real value in its environment,
  and everything works. It costs the secret: an agent can read it and send it to any host it
  reaches.

Recommended: a placeholder, swapped on the way out. It covers the keys agents use most, and it says
plainly which ones it does not cover yet.

## Notes

Which process holds the real values is the open question `which-process-holds-the-boundary` in
`engine`.

## Where a placeholder travels: a real case (24 November 2026)

An agent's part, 24 November 2026. Claude Code 2.1.331 ran with three placeholders in its
environment: `ANTHROPIC_API_KEY`, `OPENAI_API_KEY` and `GITHUB_TOKEN`, each a made-up value. The
fake model asked for one command, `env | grep -i -E "key|token"`, as an agent looking for
credentials would.

- Claude Code sent its own key's placeholder in the `x-api-key` header of every request, as
  expected.
- The command printed all three placeholders. Claude Code sent that output to the model as the
  call's result, so from then on every request to `api.anthropic.com` carried all three
  placeholders in its body, gzip-compressed. Claude Code also put a token of its own in the
  command's environment, and that token went into the conversation the same way.

Read against the answer, two things go wrong:

- **"A request that carries the placeholder to any other host is denied"** denies every later
  request of this conversation: the OpenAI and GitHub placeholders are now part of its history, and
  `api.anthropic.com` is not a host allowed for them. One `env`, or one `cat .env`, ends the
  conversation for good. The denial protects nothing, because a placeholder is worth nothing away
  from the boundary.
- **Swapping "on the way to the hosts you allowed"**, if it means anywhere in the request, puts
  real values into the model's context. Anthropic's placeholder is in the body of a request to
  Anthropic, an allowed host, so the real key would be written into the conversation. Worse, an
  allowed host can publish: an agent that posts the GitHub placeholder in the body of an issue
  through `api.github.com`, the host allowed for that token, would have the real token published in
  the issue. The body is also compressed, so a swap there means compressing it again. The request
  then no longer leaves as the program wrote it, against `secret-and-prompt-cache`.

What the agent would put in its place, for polarzero to weigh: a secret names where it goes in a
request, such as the `Authorization` or `x-api-key` header for the hosts allowed. Silo swaps it
there and nowhere else. A placeholder seen anywhere else, in a body or towards another host, is
forwarded as it is, and the record keeps it as a fact. Nothing is denied for it, since what leaves
is only the placeholder. A program that sends its key somewhere else (a query string, a body) is
found when the secret is set up, and the app says that secret cannot be swapped.

## A key that signs: AWS, signed again at the boundary (24 November 2026)

An agent's part, 24 November 2026. The answer says a key that signs requests "cannot be swapped
in". For AWS, the most common such key, a sketch shows it can be done another way: the boundary
does not swap the key into the request, it signs the request again.

The setup: the AWS CLI (1.46.3) ran with a placeholder access key id and secret, against a
stand-in for AWS. The stand-in checks every signature with botocore, AWS's own Python library,
and knows only the real key. Between them, a proxy of 54 lines, written from the SigV4
specification and not from botocore, did what the boundary would do. It read the access key id
from the `Authorization` header, swapped in the real one, rebuilt the canonical request from the
bytes on the wire, signed it with the real secret, and replaced the signature. The scripts are in
the project's files, under `research/2026-11-24-sigv4/`.

- Sent straight to the stand-in, the placeholder was refused, as expected.
- Through the proxy, every call passed: `sts get-caller-identity`, `dynamodb list-tables`, an
  S3 upload, a 20 MB upload in three parts (start, parts, finish), a download, a `HEAD`, and a
  listing whose query held a space, a slash and a `ü`.
- The real secret never entered the computer. The CLI there held only the placeholder.

What it takes, and what is still open:

- For S3, the hash of the body is in a header (`x-amz-content-sha256`), so the request can be
  signed again before its body passes. For other services, the body's hash is part of what is
  signed, so the boundary reads the whole body before it sends anything. Those bodies are small
  API calls.
- Not tried: uploads signed chunk by chunk (`STREAMING-AWS4-HMAC-SHA256-PAYLOAD`, which some SDKs
  use), where each chunk's signature chains to the one before and would be signed again in turn;
  SigV4a, which signs with a key pair derived from the secret; and a session token
  (`x-amz-security-token`), which is swapped like any value but is signed too, so it is swapped
  before signing.
- A presigned URL made inside the computer carries a signature made with the placeholder. It works
  when it is used from inside, through the boundary, and fails if it is handed to someone outside.
  The CLI's `s3 presign` still made the older SigV2 kind, which can be signed again the same way.
- The same pattern may cover other keys that sign: a Google service account key signs a token
  request that the boundary could sign again, and Azure's shared keys sign each request with HMAC
  as AWS does. A protocol that proves the key in an exchange, such as SSH or a database's
  challenge, cannot be signed again this way. The boundary would have to end that protocol itself.

So the answer's "cannot be swapped in" holds for the value, but not for the key's use. What I
would put in its place for AWS: the boundary signs the request again with the real key, on the way
to the hosts you allowed, and the computer holds a placeholder for the key as for any other
secret.

## An SSH key, held by an agent outside the computer (24 November 2026)

An agent's part, 24 November 2026. An SSH key cannot be swapped in or signed again on the way,
because SSH encrypts the whole exchange end to end. But it can stay outside the computer all the
same, the way SSH itself allows: the key lives in an SSH agent outside, and the computer gets only
a socket to that agent, `SSH_AUTH_SOCK`. OpenSSH (8.9 and later) can also bind such a key to the
hosts it may be used for, which is the same limit Silo puts on a secret.

Tried with OpenSSH 9.6, with two SSH servers on this machine, A and B, each with its own host key,
and both accepting the same user key:

- The key was added to an `ssh-agent` with a destination limit: `ssh-add -H known_hosts -h
  "[127.0.0.1]:2222" user`. The client side held no key file at all, only the agent's socket.
- `ssh` to A logged in. `ssh` to B was refused with "Permission denied (publickey)", although B
  accepts that key.
- The limit holds because the `ssh` client tells the agent which host it is talking to, signed by
  that host's key (`session-bind@openssh.com`). The agent checks it against the host key it was
  given, so a program inside cannot claim to be talking to A when it is talking to B.
- A client that does not send that message cannot use a limited key at all. curl's SFTP, built on
  libssh, was refused with the limited key and logged in with the same key added without a limit.
  Git over SSH uses OpenSSH's `ssh`, so it works.

What I would put in the answer's place for SSH keys: Silo holds the key in an agent outside the
computer, limited to the hosts you allowed, and gives the computer the agent's socket. Every
signature it makes is a fact for the record: which key, for which host. A program that does not
use OpenSSH cannot use such a key, and that is a refusal, not a leak.

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

Tried the same day with boto3 1.44.10 against the same stand-in, now over TLS as AWS is reached:

- boto3's default upload is neither a plain body nor signed chunks. It sends the body in
  `aws-chunked` framing with a checksum after it and declares the payload unsigned
  (`STREAMING-UNSIGNED-PAYLOAD-TRAILER`), over HTTP chunked transfer. The signature covers only
  the headers, so the boundary signs the headers again and lets the body stream past. Small
  uploads and an upload of 20 MB in parts passed.
- Presigned links, both SigV4 and the older SigV2 that boto3 still makes by default, passed when
  the boundary signed the query again. That holds only for a link used from inside. A link handed
  out, say in a pull request comment so a reviewer can see a screenshot, carries the
  placeholder's signature and is refused by AWS. Silo should say so when the secret is set up.
- Signed chunks were still not tried, since boto3 never sent them.

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

- **A limited key cannot sign a commit.** Many people sign their commits with their SSH key
  (`git commit -S` with `gpg.format=ssh`, shown as "Verified" on GitHub). With the key limited to
  github.com, the signature that `git commit -S` asks for was refused ("agent refused operation"),
  since it goes to no host; the same key without a limit signed. OpenSSH's own agent offers no limit
  between the two. Silo's agent, which sees what it is asked to sign, can: a commit signature
  names its purpose (`git`) in the signed data, so Silo can allow it, record it, or ask, beside
  the host limit for logging in. Otherwise the commits of an agent go unsigned. Tried the same day, OpenSSH 9.6.

What I would put in the answer's place for SSH keys: Silo holds the key in an agent outside the
computer, limited to the hosts you allowed, and gives the computer the agent's socket. Every
signature it makes is a fact for the record: which key, for which host. A program that does not
use OpenSSH cannot use such a key, and that is a refusal, not a leak.

## A key that buys tokens: a Google service account (24 November 2026)

An agent's part, 24 November 2026. A Google service account key does not sign every request, as
an AWS key does. It signs one token request, and Google answers with an access token, which then
goes on each request as a bearer token. Tried with Google's own Python library (`google-auth`
2.63) holding a key file whose private key was a placeholder, against a stand-in for Google that
knows only the real key:

- Straight to the stand-in, the token request was refused: "Invalid JWT Signature."
- Through a boundary sketch, it passed. The boundary signed the token request again with the real
  key, as for AWS.
- **The token in the reply is a new secret.** Had it passed as it came, the computer would hold a
  real token good for an hour, and the key would have been kept outside for nothing. So the
  boundary replaced it in the reply with a placeholder of its own, kept the real token outside,
  and swapped it back on the API request, which passed. The library never noticed.

So holding a key outside covers the secrets it buys too. The same goes for AWS STS
(`AssumeRole` answers with a temporary secret key), an OAuth refresh token, and a GitHub App's
installation tokens. A secret declared in Silo then has two parts: what the computer holds, and
what the boundary does with the replies that mint new secrets from it. The scripts are in the
project's files, under `research/2026-11-24-gcp/`.

## An MCP server's sign-in, held outside (24 November 2026)

An agent's part, 24 November 2026. A remote MCP server, such as Linear's or Sentry's, is signed in
to with OAuth. Claude Code 2.1.331 keeps the tokens in `.credentials.json`, under `mcpOAuth`,
inside the computer: an agent that reads the file holds your Linear account. These are your
tokens for a service, not the agent program's own login, which `logins` keeps Silo out of.

A fake MCP server with OAuth showed how Claude Code renews them. When the access token has expired
it sends `grant_type=refresh_token&refresh_token=<token>&resource=<server>&client_id=<id>` to the
token endpoint as a form, then the new access token as `Authorization: Bearer` on every MCP
request. The refresh token is in the body.

Then the computer was given two placeholders, and a boundary sketch between Claude Code and the
server held the real tokens. It swapped the refresh placeholder in the form's `refresh_token`
field, kept the tokens of the reply and sent placeholders on in their place, and swapped the access
placeholder in the `Authorization` header. `claude mcp list` connected, through two renewals in a
row with the server issuing new tokens each time. The credentials file held only the placeholders
at the end.

So the case of the Google key is general: a token request names its secret in a known field of its
body, and its reply mints new ones. Two things follow for the answer:

- The swap "in the header the secret names" also needs the form field of a token request. The
  field is fixed by OAuth (RFC 6749), so it is one more place a secret can name, not a search of
  bodies.
- A first sign-in made inside the computer, through the server's own page, ends with the same
  reply. Silo could keep any MCP server's tokens out of the computer with no step of yours, by
  reading the token endpoint that the server's own metadata names. Not tried: the first sign-in
  needs a browser.

The scripts are in the project files, `research/2026-11-24-mcp-oauth`.

## A placeholder in a password: Basic authentication (24 November 2026)

An agent's part, 24 November 2026. The answer above has Silo swap a placeholder in a header it
names. Package managers and git often send a token as the password of Basic authentication, which
is `user:password` in base64. A fake server logged what each client sent, with a placeholder as
the token:

| Client, where the token was set | What it sent |
|---|---|
| git 2.43, `~/.git-credentials` | Nothing at first; after a 401, `Authorization: Basic` of `x-access-token:<placeholder>` |
| pip 24.0 and uv 0.11, user and password in the index URL | `Authorization: Basic` of `u:<placeholder>`, from the first request |
| npm 10.9, `_auth` in `.npmrc` | `Authorization: Basic`, from the first request |
| npm 10.9, `_authToken` in `.npmrc` | `Authorization: Bearer <placeholder>`, as it is |
| cargo 1.97, a registry token | Nothing at first; after a 401, `authorization: <placeholder>`, as it is |

So in three of five the placeholder never shows as itself: base64 hides it, and how depends on
the length of the user name before it. A swap that looks for the placeholder's text in the
`Authorization` header misses git, pip, uv and npm with `_auth`, and the request goes out with the
placeholder and fails. GitHub over HTTPS is the case of most agents' first push.

What I would add to the answer: in an `Authorization: Basic` header, Silo decodes the value, swaps
the placeholder in the password and encodes it again. A placeholder can also be looked for in
base64 without decoding: all of it but a character or two at each end encodes to one of three
strings, one per offset, which a search of the request finds as cheaply as the text itself. That
also finds a placeholder in other base64 fields, such as the `auth` of Docker's `config.json`,
for the record.

## What other sandboxes do with a secret (24 November 2026)

An agent's part, 24 November 2026. Read from each product's own pages, not tried.

Keeping a key out of an agent's sandbox and putting it in at a proxy is now common: E2B, Docker
Sandboxes, Cloudflare's sandboxes, Deno Sandbox and Fly's Tokenizer all do it, and more projects
propose it. They share one shape: a placeholder in the sandbox, a list of hosts per secret, and the
value swapped into a request header.

| | Where the value goes | Signing | A reply that echoes the key |
|---|---|---|---|
| E2B | Headers, per host | No | Not described |
| Docker Sandboxes | Headers only, "not request bodies" | No; signs in to the agent's provider itself and renews | Not described |
| Cloudflare | Headers, in code you write per host | Not described | Not described |
| Deno Sandbox | Not said, per host | Not described | Not described |
| Fly Tokenizer | Headers | HMAC of parts of a request | Not described |

What none of them describes, and the trials of this note did:

- **Signing again.** None signs an AWS request again. Without that, an AWS key, and Claude Code on
  Bedrock, stay inside the sandbox or do not work.
- **A key that buys tokens.** None signs a Google service account's request for a token and swaps
  the token on the way back. Docker comes nearest, by signing in for the agent's provider itself.
- **The reply.** None says it searches replies for the key. One proposal (carapace, issue 561)
  writes that "an upstream echoing the token defeats the scheme" and leaves it out.
- **SSH.** Docker forwards an SSH agent and keeps the keys on the host, with no limit on where a
  key works. The binding of the trial above limits it to the hosts allowed.

Two things to avoid, seen in them:

- **E2B fails open.** When a secret cannot be found, "the request is still forwarded", without the
  header. Silo should refuse the request with its reason.
- **Docker hands the token in.** An agent kit may set OAuth `passthrough: true`, which "sends the
  real token response into the sandbox". The key then sits in the sandbox after all.

So the placeholder in a header is what every product has. What Silo would add is the rest: keys
that sign, keys that buy tokens, SSH, and the reply. Those are the cases a developer's real keys
are in (AWS, Google Cloud, a Git host over SSH), and the reason to pick Silo over the others.

## Codex and Claude Code ship the same design, inside the program (24 November 2026)

An agent's part, 24 November 2026. Read from Codex 0.172.0's source (`codex-rs/network-proxy`, its
README and `credential_broker/`), not run. Codex now carries a network proxy of its own for the
commands it starts, experimental and off by default ("Network proxy can now be enabled from
/experimental"). With `credential_masking` on, it does most of what this answer proposes:

- Commands get a stand-in for `GITHUB_TOKEN`, `GH_TOKEN`, `OPENAI_API_KEY` and the like, shaped
  like the real one (same prefix, at least the same length), and the proxy swaps the real value
  into `Authorization` for the hosts of that provider only. For GitHub it decodes Basic and swaps
  the user name or the password, the case found above.
- AWS: a stand-in key id such as `CODEXMASKEDWORKSPACE`; the proxy finds it in the signature's
  scope and signs the request again with the real key, keeping the client's timestamp and signed
  headers. A body with `UNSIGNED-PAYLOAD` streams; a signed body is held, up to 8 MiB.
- Other secrets: a header set from a secret, which can require one exact stand-in value first, and
  `body_secret_substitution` with exact markers of 16 to 256 bytes. A compressed body fails closed.
- Hooks per host, method and path, a mode that allows only `GET`, `HEAD` and `OPTIONS`, and a
  certificate authority made per run, whose key stays in the proxy's memory.

Claude Code 2.1.331 has the same in its own sandbox, read from the settings it documents in its
binary: an environment variable or a credential file set to `mask` "substitutes a sentinel inside
the sandbox … and injects the real value at the proxy"; the usual AWS trio is paired for "SigV4
re-signing"; named claims of a JWT can be masked and the token rebuilt around them; masking
requires the proxy to end TLS; and "On macOS and Windows `mask` degrades to `deny`" for files.

What Codex refuses, and what Silo's trials passed: presigned links with a masked key, SigV4a, AWS
signed streams and "declared trailers". boto3 1.44's default upload declares a trailer
(`STREAMING-UNSIGNED-PAYLOAD-TRAILER`), so it fails under Codex's proxy; re-signing the headers
let it through in the trial above. Neither the reply of a token request nor SSH is covered.

What this changes for Silo:

- **The design is not in question.** Both vendors reached the same shape: a stand-in, a swap at
  a proxy that ends TLS, host binding, signing again for AWS, failing closed. Silo can borrow its
  limits (exact markers, no escaping, a size cap on held bodies) rather than invent them.
- **Where the real value lives is the difference.** Codex's proxy runs on the same machine as the
  agent, and its secrets are files or variables there; "file-backed credentials require filesystem
  permissions that prevent child access". With the sandbox off, the way polarzero's first build
  runs agents, Codex says it plainly: "Credential masking is inactive because the network proxy is
  disabled. Commands will receive unmasked credentials." Claude Code's
  masking too exists only inside its sandbox. Silo's boundary is outside the computer, so it holds
  with every check off and for every program.
- **Tried: Claude Code's sandbox with every check off.** Claude Code 2.1.331 on Linux, its sandbox
  on, `GITHUB_TOKEN` and a file set to `deny`, run with `--dangerously-skip-permissions`, as
  polarzero's first build runs it. The fake model ran one command twice. In the sandbox it got
  `token=[]` and "Permission denied". The second time it asked for `dangerouslyDisableSandbox:
  true`, and with no question asked the command ran outside and printed the real token and the
  file. Its own instructions to the model say to retry this way after a failure that looks like
  the sandbox's. With `allowUnsandboxedCommands: false` the second command stayed in the sandbox
  and got nothing. So the vendor's protection holds with every check off only when that one
  setting is off too, and it is on by default; Silo's holds whatever the program is told.
- **A stand-in shaped like the real token** is worth taking: some tools check a token's prefix or
  length before they send it. The answer above does not say what a placeholder looks like.
- **Two proxies in a row.** Codex with its proxy on, inside Silo, ends TLS itself and reaches the
  host through Silo's boundary. Its proxy trusts "native root certificates for MITM trust bundle",
  so Silo's authority in the computer's store should be enough. Not tried.

## A database password, held outside (24 November 2026)

An agent's part, 24 November 2026. The answer leaves database passwords as ordinary files "until
a way is found", and the note on AWS says the boundary "would have to end that protocol itself".
That was tried for PostgreSQL, the database agents meet most, with PgBouncer 1.22 standing in for
the boundary. The server was PostgreSQL 16 with TLS and SCRAM-SHA-256, the default of hosted
databases, so the password itself never crosses the connection and no swap can work.

- **Ending the protocol works.** The computer's `DATABASE_URL` held a placeholder as the password.
  The stand-in checked the placeholder, then logged in to the server itself with the real
  password, over TLS it verified. psql 16, psycopg 3.3, asyncpg 0.32, Node `pg` 8.23 and Node
  `postgres` 4.1 all connected and ran queries, prepared statements and a cancelled query worked.
  The placeholder sent straight to the server was refused.
- **It needs no certificate the computer trusts in the usual case.** psql connected with
  `sslmode=disable` and with `require`, the mode hosted databases print in their connection
  strings, which encrypts without checking the certificate. `verify-full` needs Silo's authority,
  as HTTPS does.
- **It costs channel binding.** A client that demands it (`channel_binding=require`) was refused,
  because PgBouncer 1.22 does not offer it to its clients. Silo's own end of the protocol would
  have to offer it to keep such a client working.
- **It is a proxy of a whole protocol, not a swap.** Silo keeps one connection to the server for
  each connection from the computer, and must answer the start-up, the cancel requests and the
  errors itself. PgBouncer does all of it, in session mode, so it could serve as the part that
  ends PostgreSQL, or show what Silo's own needs to do. MySQL, which has its own challenge, was
  not tried.
- **Redis needs only a swap.** Its password travels as it is, in the `AUTH` command (or `HELLO`).
  A stand-in that swapped the placeholder in the command stream worked for redis-cli 7.0,
  redis-py 8.2, ioredis 6.1 and node-redis 6.4, in both versions of the protocol. The swap has to
  fix the length written before the value, since the protocol counts bytes, unless the placeholder
  has the real value's length (the shaped placeholder above). Under TLS (`rediss://`, as hosted
  Redis gives) Silo ends TLS first, as for HTTPS.

What I would add to the answer: PostgreSQL passwords can be held outside, by a boundary that ends
the protocol, which removes the most common secret from the "ordinary files" list. The
configuration is in the project files, `research/2026-11-24-db-password`.

## Where twelve services' secrets travel (24 November 2026)

An agent's part, 24 November 2026. The answer covers secrets "sent as they are". To see how many
of the services agents use send them so, and where, each service's own Python SDK made one call
with a placeholder key, to a server standing in for the service, and the request was searched for
the placeholder.

| Service, SDK | Where the placeholder was |
|---|---|
| Stripe 17.1, Hugging Face 2.4, Cloudflare 5.10, SendGrid 6.12, Resend 2.55, PostHog 8.2, Slack Web API 3.48 | `Authorization`, in plain |
| Supabase 2.34 | two headers, `apiKey` and `Authorization` |
| Sentry 3.0 | `X-Sentry-Auth`, and again in the body, which was gzip-compressed |
| Twilio 9.13 | `Authorization: Basic`, base64 of the account and the token |
| Slack incoming webhook | the URL path: the webhook's address is the secret |
| Azure Blob Storage 12.31, with an account key | nowhere: the request carries `SharedKey account:<signature>`, made with the key inside the computer |

What this shows:

- **Most services fit the swap as the answer has it.** Eight of the twelve send the key in plain
  in a header. A swap that replaces every place the placeholder appears in the headers serves
  Supabase's two.
- **The other four are the cases already raised.** Twilio needs the Basic decoding proposed above.
  The Slack webhook needs a secret that declares its place in the URL. Azure's shared key signs
  each request as AWS does, so it needs signing again at the boundary. Sentry's key is public by
  design, so its copy in a compressed body matters less, but it shows that a placeholder can travel
  in a body the swap does not decode.
- **Stripe's Python SDK refused Silo's certificate.** It ships its own list of authorities and
  reads no variable; only code that sets `stripe.ca_bundle_path` makes it trust another. With
  that set, its key went in plain in `Authorization`. Stripe is among the APIs agents call most,
  so this is the clearest case of a program that fails on a host Silo reads
  (`ending-the-encrypted-connection`).

The scripts are in the project files, `research/2026-11-24-where-secrets-travel`.

## Kubernetes: a cluster's own authority, and EKS tokens (24 November 2026)

An agent's part, 24 November 2026. Agents that deploy reach a Kubernetes cluster with a
kubeconfig, which holds both the way to sign in and the cluster's own authority.

- **A kubeconfig pins the cluster's authority.** The Python `kubernetes` client 37.0 refused a
  stand-in API server whose certificate the system store trusted, because the kubeconfig named
  another authority, as every cluster's does. With the kubeconfig naming the stand-in's authority,
  it connected. So Silo cannot read or hold a cluster's API calls, nor swap a token in them, unless
  the computer's kubeconfig names Silo's authority. Silo can arrange that when the kubeconfig is
  brought in as a secret: the computer gets a copy with Silo's authority and placeholders, and
  Silo checks the real cluster against the real authority. kubectl was not tried; downloading it
  was not allowed on the test machine.
- **An EKS token can be signed again.** The token `aws eks get-token` makes is a presigned AWS
  request to STS, wrapped in base64, that also signs the cluster's name. It crosses the boundary
  as a bearer token to the cluster, not as a request to AWS. Made with a placeholder key, unwrapped,
  signed again with the real key and wrapped again, it was byte for byte the token the real key
  makes at the same second. So the AWS re-signing above extends to EKS, once Silo knows the shape.
- **A client certificate in a kubeconfig** (kind, k3s and some managed clusters) proves the key in
  the TLS handshake. A boundary that ends TLS presents the real certificate to the cluster itself;
  this was not tried.

What I would add to the answer: a secret can be a whole file Silo rewrites for the computer (a
kubeconfig with placeholders and Silo's authority), not only a value. The scripts are in the
project files, `research/2026-11-24-kubernetes`.

## A placeholder stored as another service's secret (24 November 2026)

An agent's part, 24 November 2026. An agent that sets up a deployment copies your keys into the
service that runs it: GitHub Actions secrets, Vercel or Cloudflare variables, Fly secrets. In a
computer with placeholders, what it copies is the placeholder. Tried with `gh secret set
OPENAI_API_KEY` (gh 2.89) against a stand-in for GitHub's API:

- gh asked for the repository's public key, sealed the value with it inside the computer, and
  sent only the sealed box. The stand-in, holding the private key as GitHub does, opened it and
  found the placeholder.
- So the boundary cannot swap it: it never sees the value. It should not swap it either way,
  since the real key would then leave for a host the secret was not allowed to reach.
- The deployment then fails later, far from the computer, with an invalid key, and nothing told
  you why. A service whose API takes the value in plain shows the placeholder to the boundary,
  which can recognise it (Vercel's, Cloudflare's and Fly's are documented so; not tried); GitHub's sealed box does
  not, though the request names the secret (`PUT …/actions/secrets/OPENAI_API_KEY`).

What I would add to the answer: a request that stores a secret in another service is a moment to
tell you, or to ask. Silo can recognise the known ones by their address, and name the secret.
Asked, it could store the real value itself: for GitHub it would seal the real value with the
public key it saw in the reply before. That puts your key in a second place, which is your call,
not the agent's. The script is in the project files, `research/2026-11-24-secret-stored-elsewhere`.

## Azure's shared key, signed again (24 November 2026)

An agent's part, 24 November 2026. The note on AWS says Azure's shared keys sign each request with
HMAC as AWS does, and twelve services showed Azure Blob sending only a signature. Tried with
azure-storage-blob 12.31 and Azurite 3.38, Microsoft's storage emulator, which checks shared-key
signatures as Azure does, set up with a real key.

- With a placeholder key, the emulator refused every request ("Server failed to authenticate the
  request").
- Through a stand-in that signed each request again with the real key, built only from what was on
  the wire (the method, a dozen standard headers, the `x-ms-` headers, the path and the query), the
  same client created a container, uploaded a blob whose name has a space, listed, downloaded and
  deleted it. The stand-in is 40 lines.

So Azure Storage joins AWS, Google service accounts and EKS: the key never enters the computer,
and the boundary signs. Links signed inside the computer (shared access signatures) were not
tried; like AWS's presigned links, they would work only through the boundary. The script is in the
project files, `research/2026-11-24-sigv4` (`resign_azure.py`).

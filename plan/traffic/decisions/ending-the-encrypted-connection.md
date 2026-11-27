# How does the boundary read encrypted model traffic

## The choice

An agent's part, 25 November 2026, from the challenge of #355 (page 2), with #363 and #356.

The case: github.com is allowed so that agents can clone public repositories, and no GitHub secret
is given. A worm in the computer pushes what it stole to its own repository, with its own token.
Under the earlier answer that connection "passes encrypted and unread", so no rule can see the
token, let alone refuse it.

The options:

- A rule can ask the boundary to read a host it allows, and each request must name the host of its
  handshake (recommended). With `foreign-credential-on-a-secret-host`, a rule on a host read this way can refuse a
  credential that is not Silo's. #363 and #356 both found that a request naming another host than
  its handshake has to get no secret; over HTTP/2 the answer is the status 421 (#355). It costs the
  encryption of the hosts such a rule names, which Silo then reads.
- Read only the hosts the earlier answer lists. It costs the worm's case.

Recommended: a rule can ask to read a host. The default does not change: a host is read only when it
is a provider's, a registry a rule names, one that gets a secret, or one a rule names.

## The earlier choice

The choice as it stood before 25 November 2026. The answer it recommended is now an option of the choice above.

An agent's part, 22 November 2026. Claude Code in main talks to api.anthropic.com. An install script
fetches from registry.npmjs.org, which your rule on package age names. An agent clones a repository
from github.com.

The options:

- Only the hosts Silo needs (recommended). Silo reads the connections to api.anthropic.com and
  registry.npmjs.org. The clone from github.com passes encrypted and unread, and the record keeps
  its host, size and timing. Nothing reaches a host Silo reads without being read: a program that
  tries HTTP/3 there is refused and falls back to an ordinary connection. Silo checks each real
  host's certificate itself, so nothing inside main can make it trust a false api.anthropic.com. It
  costs what Silo does not read: the record shows that main reached github.com, not which files it
  fetched. A host Silo reads whose certificate comes from your company's own authority also fails,
  with no way yet to make Silo trust it.
- Every encrypted connection. Silo reads all of main's traffic, as a company proxy set to inspect
  everything does, and the record holds the address of every page an agent opened. It costs every
  program in main depending on Silo's certificate: one that refuses it fails on every host, not on a
  few. It also costs a wait on each connection, more disk for the record, and whatever travels in
  those addresses, such as a token in a link, kept in the record.

Recommended: only the hosts Silo needs. Each host Silo reads is a place where a program can refuse
its certificate and break, so it should be only where reading gives something: spend, tool calls, a
package's age, a secret.

## The Host of each request (24 November 2026)

An agent's part. The strict reader in `research/2026-11-24-red-team-fixes/strict/` refuses a request
whose `Host` is not the name in the handshake, or whose target is an absolute URL. Go's `net/http`
takes the host from an absolute URL over the `Host` header, so a request sent under
`api.anthropic.com` could be served as `attacker.example`. A suggestion for the answer: each request
on a connection Silo ends names the host of the handshake, and is refused otherwise.

## Containers inside a computer do not trust Silo's certificate (24 November 2026)

An agent's part, 24 November 2026. Tried on Linux with Docker 29.9.3 and `node:22-alpine`. A fake
host presented a certificate from a test authority that the machine's own store trusts, as the
computer would trust Silo's.

- From a container, Node refused: `SELF_SIGNED_CERT_IN_CHAIN`. `wget`, which reads the image's own
  store, refused too.
- `docker build` of a `RUN wget …` step failed the build: "certificate verify failed".
- With the authority mounted into the container and `NODE_EXTRA_CA_CERTS` pointing at it, Node
  passed.

A container brings its own list of authorities, so whatever the computer trusts stops at its edge.
`docker build` with `npm install` or `pip install` inside is common work for an agent, and those
registries are hosts Silo reads for the age rule (`registry-traffic-read`). Every such build then
fails, with an error that names a certificate and not Silo.

What I would add to the answer:

- **Fail closed, and say why.** A client that refuses Silo's certificate must not get the host
  unread instead, or any agent gets around a rule by using a client that does not trust Silo. The
  record says that a program in the computer refused Silo's certificate for that host, and the app
  shows it beside the failed command.
- **Make the fix one line.** Silo keeps its authority at a fixed path in every computer and tells
  the agent how to use it in a build: copy it into the image and run `update-ca-certificates`, or
  pass it as `NODE_EXTRA_CA_CERTS`, `PIP_CERT`, `SSL_CERT_FILE`. Whether a container runtime in the
  computer can add it to every container by itself is to find out.

Tried the same day: it can, for containers that run. A 20-line wrapper around runc, set as Docker's
default runtime in the computer, adds the computer's bundle of authorities over the image's own,
read-only, and the variables that point libraries at it, unless the container sets them itself.
With it, Node's `fetch` and `wget` in `node:22-alpine` trusted Silo's certificate, a public host
still worked, and nothing changed in the image or the command. `docker build` is not covered: its
default builder (BuildKit) runs build steps without Docker's default runtime, and a `RUN wget` in a
build still refused the certificate. So containers can be made to trust Silo by Silo itself, and
image builds still need the line above. The wrapper is in the project files,
`research/2026-11-24-container-trust`.

Docker itself trusts the computer's store: a registry whose certificate came from an authority
found only in Ubuntu's system store took `docker push` and `docker pull` (Docker 29.9), and
`docker build` fetched its `FROM` image from it, with no setting. So on a registry host Silo
reads, pulling images works as it is; only the commands that run inside a container or a build
step need Silo's authority given to them.

## Libraries and browsers that keep their own store (24 November 2026)

An agent's part, 24 November 2026. `registry-traffic-read` found which package managers trust
the computer's own store. Agents also run their own code and browsers through Silo, so the same
test was run on the libraries that code uses: a server with a certificate from an authority found
only in Ubuntu's system store, and each client with no certificate setting.

| Trusts the system store | Refuses Silo's certificate | What makes it trust the system store |
|---|---|---|
| Python `urllib` and `aiohttp` 3.14; the OpenAI 3.58 and Anthropic 2.13 SDKs, through `truststore` | Python `requests` 2.34 and `httpx` 0.28: their own bundle (`certifi`) | `REQUESTS_CA_BUNDLE`, and `SSL_CERT_FILE` for httpx, set to the system bundle |
| Ruby `net/http`, PHP, Java (Ubuntu fills its store), curl | boto3 1.44 | `AWS_CA_BUNDLE` |
| | Node 22 `fetch`, `https`, axios | `--use-system-ca`, as already found |
| | Chromium 158 from Playwright, and Google Chrome: `ERR_CERT_AUTHORITY_INVALID` | the authority added to the user's `~/.pki/nssdb`, the store Chrome reads on Linux |

What this changes:

- **An agent's own Python script breaks in the most common way.** `requests` is the HTTP library
  Python code reaches for first, and an agent that writes a script with it gets "certificate
  verify failed" on every host Silo reads. Each variable above replaces the library's bundle with
  the file it names, so it must name the whole system bundle, not Silo's authority alone, or
  every other host fails instead.
- **A browser the agent drives does not read the system store.** Agents test web apps with
  Playwright; on a host Silo reads, Chromium refuses the page until Silo's authority is in the
  NSS store of the user that runs it. Added there, both Chromium and Chrome trusted it.
- So the list of places a computer needs Silo's authority grows to: the system store, Node's (by
  the environment), uv's, `requests`, `httpx` and boto3's (by the environment), and each user's NSS
  store for browsers. Silo can set all of these when it makes a computer; what it cannot set is a
  library that ships its own roots and reads no setting, and that program then fails on the hosts
  Silo reads, as a container does above. Stripe's Python SDK is one: only a line of code changes
  its list (`secret-placeholder`, "Where twelve services' secrets travel").

All of it together was tried from a fresh home folder: six variables pointing at the system
bundle (`NODE_OPTIONS=--use-system-ca`, `UV_NATIVE_TLS=1`, `SSL_CERT_FILE`, `REQUESTS_CA_BUNDLE`,
`AWS_CA_BUNDLE`, `CURL_CA_BUNDLE`) and Silo's authority added to the user's NSS store. Every
client above then trusted Silo's certificate, Chromium and Chrome included, and a real public
host still worked. That is a short, fixed list Silo can apply when it makes a computer.

The scripts are in the project files, `research/2026-11-24-trust-libraries` (`kit.sh` for the
list).

The agent programs themselves, for their own model traffic, tried the same day against a stand-in
for each provider's host with a certificate from an authority only the system store held:
Claude Code 2.1.331 (built on Bun) and opencode 1.19.6 (Bun 1.4.4) trusted it, as Codex 0.172.0
already had. Gemini CLI 0.71.1, which runs on Node, refused it, and said only "fetch failed
sending request", twice with retries, nothing about a certificate. With
`NODE_OPTIONS=--use-system-ca`, the first line of the list above, it reached the stand-in. So the
list covers every agent program tried, and without it Gemini CLI fails in a way that names
neither the certificate nor Silo.

## What a certificate Silo makes must hold (24 November 2026)

An agent's part, 24 November 2026. The answer has Silo present "a certificate it makes for that
host" and says the settings "are written down and tested, not left to defaults". The spike used
one certificate for every host. Certificates were made per host with Python's `cryptography` in
five shapes, under one test authority, and four clients connected to each, trusting only that
authority:

| Leaf certificate | Python 3.13 | Node 22 | curl 8.5 | Go 1.24 |
|---|---|---|---|---|
| ECDSA P-256 key, the host in `subjectAltName`, Authority Key Identifier | ok | ok | ok | ok |
| The same without Authority Key Identifier | refused: "Missing Authority Key Identifier" | ok | ok | ok |
| The host only in the common name, no `subjectAltName` | ok | ok | ok | refused: "relies on legacy Common Name field" |
| An RSA 2048 key | ok | ok | ok | ok |
| Valid for 800 days | ok | ok | ok | ok |

- **Python 3.13 checks strictly by default.** Its default context sets `VERIFY_X509_STRICT`, so a
  leaf without the Authority Key Identifier extension fails every Python program of 3.13 or later
  with "certificate verify failed", whatever store it trusts. A library that makes certificates
  may leave that extension out unless asked, and Node, curl and Go would not notice, so only a
  test with a recent Python catches it.
- **The host goes in `subjectAltName`.** Go refuses a certificate that names its host only in the
  common name, as Chrome does (Chrome not tried here). gh, kubectl, terraform and docker are Go.
- **Making one costs nothing with an elliptic-curve key.** A leaf with a new P-256 key took
  0.27 ms to make and sign; with a new RSA 2048 key, 70 ms, nearly all of it making the key. So
  Silo can make each host's certificate at the first connection to it, with no cache to keep
  warm, if its keys are elliptic-curve.

Python 3.13's strict check reaches the authority too: an authority made by `openssl req -x509`
with no key usage extension was refused ("CA cert does not include key usage extension"), found
while trying a client certificate (`secret-placeholder`, the Kubernetes section).

What I would add to the answer: Silo's authority has `basicConstraints` and `keyUsage`, marked
critical; each certificate Silo makes has an ECDSA P-256 key, the host in
`subjectAltName`, and the Authority Key Identifier and Subject Key Identifier extensions, and a
test connects Python 3.13, Node, curl and Go to it, since each refuses a different omission. The
script is in the project files, `research/2026-11-24-trust-libraries/mint`.

## Notes

An agent's part, 25 November 2026 (`research/2026-11-25-boundary-end-to-end`). Reading any host a
secret goes to needs a certificate per name. The boundary made each on first use, in under 1.5 ms,
and kept it. Python 3.13 refuses a certificate without an authority key identifier, which OpenSSL
adds only when asked. Each certificate starts a day back, so a computer whose clock fell behind
takes it, and Silo's authority must start in the past too: one made at setup was refused by a
computer 2 h behind.

The boundary of #347 and #374 offers only HTTP/1.1. curl and Node fall back. A gRPC client
cannot, so #379's swap inside HTTP/2 headers has nowhere to run until the boundary speaks HTTP/2
on the hosts it reads that serve gRPC.

An agent's part, 25 November 2026 (`research/2026-11-25-boundary-conflicts`). The boundary now
speaks HTTP/2 on hosts Silo reads that are marked for it, with nghttp2 on both sides. A gRPC client
in each computer called a Google API with its placeholder: one call and one stream with trailers.
Both reached the host with the real token. Before the change every call failed in the TLS handshake,
since the boundary offered only HTTP/1.1. Each request's headers go through the same checks and swap
as on HTTP/1.1. A refused gRPC call gets `PERMISSION_DENIED` with Silo's sentence, which the client
prints. Model hosts stay on HTTP/1.1, where #374 counts calls. Not built yet: offering HTTP/2 only
where the host offers it, which needs the boundary to reach the host before it answers the program.

An agent's part, 25 November 2026 (`research/2026-11-25-tls-refusals`). Each host Silo reads is
where a program can refuse Silo's certificate, and the boundary is the only place that sees it.
The program's ClientHello names its TLS client well enough to tell programs apart: Claude Code,
Codex, curl, Python, Go, git and Node each had their own, Claude Code and Codex two each. A
fingerprint names a TLS library build, not a program, so Silo learns the name from the
User-Agent of a connection that worked.

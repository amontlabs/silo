# SSH keys and cloud credentials through the boundary, measured

Research of 25 November 2026, by an agent, for polarzero. Draft PR #356 proposed three answers in
`ssh-keys`, `exchanged-tokens` and `cloud-credentials`, from the products' documentation. This
folder builds each one as a small prototype on Linux and runs real programs against it: OpenSSH
9.6, Paramiko, asyncssh, Go's `x/crypto/ssh`, Git, go-git and cargo for SSH; `google-auth`,
`google-cloud-storage`, `gcloud` 570, `azure-identity` 1.27, `msal` 1.39 and `az` 2.93 for the
clouds; gRPC in Python, Go and Node for HTTP/2.

The Mac is played by the boundary process, which holds every real key. The computer is a set of
processes with their own home directory, their own `known_hosts`, and no private key or cloud key
file. Google and Microsoft are played by local servers that accept only real credentials and
refuse placeholders (`cloud/fake_clouds.py`). No real cloud account was used.

## What was found

**1. The SSH agent at the boundary works, and it does not need clients to bind.**
OpenSSH 9.6 logged in, cloned, pushed and signed a commit through the agent. The agent refused
every case it should refuse: a host not on the key's list, a host key the boundary does not know,
a server the computer reached without crossing the boundary, the relay attack from #356, a sign
request with no connection behind it, a file signature outside the `git` namespace, and `ssh -A`
onward from the server. Plain agent forwarding, as Docker Sandboxes does it, allowed all of them.

#356 expected to refuse clients that do not send `session-bind@openssh.com`. Paramiko, asyncssh
and Go's `x/crypto/ssh` were refused that way. The data a client asks the agent to sign holds the
session id, and the boundary saw the host sign that same session id in the clear. So the boundary
can find the connection itself: it checks the host's signature over that session id against each
open connection it carries. With that check, all three clients worked, and every attack above was
still refused. The cost is one signature check per open SSH connection, about 87 µs each here.

**2. A tunnel defeats the relay check.** An outsider who can reach a program in the computer (a
WebSocket to an allowed host is enough) can have that program open the TCP connection to
`github.com`, and carry the outsider's own SSH session through it. The boundary sees a real
connection from the computer, to an allowed host, with the right host key. The agent signs. The
outsider holds the session keys and is logged in as you. This worked against the prototype. The
check stops a signature being used for a connection Silo never carried; it cannot tell who is at
the computer's end of a connection it did carry. Only a boundary that ends SSH itself (#356's
second option) sees what the session does, and even then not who drives it.

**3. The HTTPS rewrite for Git covers Git and cargo, not go-git.** With
`url."https://github.com/".insteadOf git@github.com:` in the Git config, `git` and cargo
(libgit2) fetched `git@github.com:octocat/Hello-World.git` over HTTPS. go-git 5.16 ignored the
rewrite and asked for an SSH agent. With the agent of finding 1, go-git's SSH still works.

**4. The metadata server with placeholders worked for every Google and Azure program tried, and
nothing real reached the computer.** With no setting in the computer, `google.auth.default()`
found the server and listed a bucket. `gcloud storage ls`, an ID token for Cloud Run,
`ManagedIdentityCredential`, `DefaultAzureCredential`, `az login --identity`, `az group list` and
`az account get-access-token` all worked. A program that sent every file of the computer's home
and fresh tokens from the server to an outside host sent only placeholders. Each placeholder, sent
straight to the cloud from outside, was refused.

The same run with real one-hour tokens (#356's "first cut") worked the same. Every token the
computer got was usable from outside for its life: 3 Google access tokens, 3 Google ID tokens and
7 Azure access tokens reached the computer. `gcloud` wrote one to `access_tokens.db` on disk.

**5. A token that is a JWT needs a placeholder that is a JWT, for Google too.** #356 said Azure's
tokens are JWTs that programs decode. So are Google's ID tokens. With an opaque placeholder,
`google.oauth2.id_token.fetch_id_token` failed ("Wrong number of segments in token") and
`az login --identity` failed ("Not enough segments"). A placeholder made of the real token's
header and claims, with another signature, worked everywhere. Its claims are not secret, but they
name the account (an email, a tenant, an object id).

**6. Letting an exchanged token through can hand over the long-lived secret itself.** Microsoft
rotates refresh tokens: each refresh returns a new one and spends the old. With the answer passed
through, the computer's first refresh put the new real refresh token in its home directory, and
the refresh token Silo held stopped working (HTTP 400). The computer then held the only live
credential, valid for months, not an hour. With the answer rewritten, the computer refreshed three
times with one placeholder that never changed, and Silo kept each new real token.

**7. On HTTP/2, the swap must decode headers.** gRPC clients put the placeholder in an HPACK header.
Python's `grpcio` sent it as plain bytes, so a swap of bytes worked. Go's `grpc` and Node's
`grpc-js` compressed it with HPACK's Huffman code: the placeholder's bytes never appeared in the
stream, a swap of bytes changed nothing, and the server received the placeholder. A boundary that
decodes each HEADERS frame, swaps the value and encodes it again worked for all three, in about 70
lines on the `h2` library.

## What it changes in the plan

Notes are added to each entry, as an agent's part. No verdict is written.

| Entry | Change |
|---|---|
| `ssh-keys` | Clients that do not bind are no longer refused: the boundary finds the connection from the session id. The tunnel case is added, and it narrows the note that says the relay fails. `ssh -A` onward is refused, measured. |
| `exchanged-tokens` | Measured working for Google's and Microsoft's endpoints, with refresh-token rotation. Challenge to the fallback: an unknown endpoint that passes its answer through can hand over a rotating refresh token, and kill Silo's own copy. Proposed: a placeholder is swapped into a `refresh_token` field only at a known endpoint. Elsewhere the request is held with an ask. |
| `cloud-credentials` | Measured working with placeholders for every program tried. Challenge to "until that swap is built, the computer would get the real token": the swap on HTTP/1.1 is the swap that exists; only gRPC needs header decoding, which is small. Proposed: no first cut with real tokens. A JWT placeholder is needed for Google's ID tokens as well as Azure's tokens. |

## For polarzero to rule

- `ssh-keys`: accept that a connection the computer opened can carry someone else's session. The
  agent's limit is "for connections that cross the boundary, to hosts on the key's list, with a
  host key Silo checked". Seeing what a session does needs the boundary to end SSH (#356's second
  option), at the cost of an SSH server and client in the boundary.
- `exchanged-tokens`: hold, instead of pass, a refresh at a token endpoint Silo does not know.
- `cloud-credentials`: build placeholders from the start, and skip the real-token first cut.

## How to run it

Each prototype is a script that sets up its servers, runs every case in each mode and prints one
line per case. Both need root on Linux. Neither reaches the internet, except the Git rewrite test.

| Folder | What | Run | Output |
|---|---|---|---|
| [`ssh/`](ssh/) | `silo_ssh.py` is the boundary: a port-22 relay that reads each key exchange, and the agent. `run.py` starts two `sshd` and runs 18 cases in the modes `silo`, `silo+infer` and `plain` | `python3 ssh/run.py` | [`ssh/results.txt`](ssh/results.txt) |
| [`ssh/insteadof/`](ssh/insteadof/) | Which Git clients follow the HTTPS rewrite | `sh ssh/insteadof/run.sh` | [`results.txt`](ssh/insteadof/results.txt) |
| [`cloud/`](cloud/) | `boundary.py` is the metadata server and the TLS end for cloud hosts. `fake_clouds.py` plays Google and Microsoft. `run.py` runs 15 cases in the modes `real-tokens`, `placeholders` and `opaque-placeholders`, then counts every real value in what the computer received, its files, and what an outsider collected | `python3 cloud/run.py` | [`cloud/results.txt`](cloud/results.txt) |
| [`cloud/h2_swap.py`](cloud/h2_swap.py) | A swap of bytes and a swap of decoded headers, under three gRPC clients | `python3 cloud/h2_swap.py <go-client> <node-client.js>` | [`cloud/h2_results.txt`](cloud/h2_results.txt) |

The cloud prototype edits `/etc/hosts` for the cloud names and puts it back, and adds
169.254.169.254 to the loopback interface.

## Not verified

- No real Google or Azure account. The stand-ins answer like the real servers for the calls these
  programs made, as read from their traffic. The real servers may check more.
- Programs not run: the Java, Go, Node and .NET cloud libraries, Terraform, `gsutil`, `bq`, Google's
  credential helper for Docker (`gcloud auth configure-docker`). It sends the access token as the
  password of Basic auth, a slot the swap must cover.
- SSH: only ed25519 keys at the agent; RSA and ECDSA host keys are checked by the code but were not
  exercised. JGit was not run. OpenSSH before 8.9 was not run; it sends no bind, so it takes the
  same path as Paramiko.
- The commit-signing case signs a hash. The agent cannot see which commit it signs, so the record
  cannot name it.
- Throughput and latency at load. One login through the boundary took 0.18 s in both modes.

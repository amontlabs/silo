# Secrets that are not sent as they are

Research of 24 November 2026, by agents, for polarzero. `secret-placeholder` says that a key that signs requests, an SSH key and a database password cannot be swapped in, and that how they are handled is to find out. Draft PR #355 already measured AWS signing, Git and `gh` over HTTPS, npm and Postgres with real programs. This page covers the four kinds it left: SSH keys, tokens obtained by exchange, Google Cloud and Azure credentials, and MySQL. Everything comes from the products' own pages, read that day. Nothing here is a decision: the choices are in `ssh-keys`, `exchanged-tokens` and `cloud-credentials`. Each section has a table of paths, one recommendation, and its sources. Five build-first bullets are at the end.

Two facts shape most of it:
- **Prior art covers only part of this.**
  - Claude Code's sandbox masking has no SSH support. Its fix for git over SSH is "switch the remote to HTTPS". Its masking only works for HTTP clients: "most database drivers, plain `ssh`, and tools that use UDP" can't connect through its proxy.
  - Managed Agents vaults state: "Substitution is outbound only… the returned token arrives in the sandbox unredacted. For exchange-based flows, perform the exchange yourself and store the resulting token."
- **Silo has a lever these products lack.** Its boundary is the VM's whole network stack, not a proxy that tools opt into. So it can see raw TCP, read the SSH key exchange (which travels in clear before encryption starts), answer `169.254.169.254`, and answer DNS.

---

## 1. SSH private keys (git over SSH, ssh to servers)

**How SSH auth works, and why a placeholder can't work.** The client signs a userauth blob that contains the session identifier. That identifier is the exchange hash `H` from the key exchange, unique to each connection. A signature made with a fake key is useless, and the boundary can't recompute it because `H` depends on the Diffie-Hellman secret. So the real private key has to sign somewhere: inside the VM, at the boundary on the VM's behalf, or at a boundary that authenticates on its own.

**Prior art**
- **Docker Sandboxes** forwards the host agent: "SSH agent forwarding is enabled by default… forwards that agent into the sandbox and sets `SSH_AUTH_SOCK`… Processes inside the sandbox can request signatures from the forwarded agent, but they can't read or copy a private key." Settings: `ssh.agentSocketPath`, `ssh.agentForwardingEnabled`. The kit `github-ssh` pre-fills GitHub's host keys, and the kit `git-ssh-sign` handles commit signing. There is no per-host policy.
- **1Password SSH agent** asks per key and per process: "The authorization prompt indicates which process is requesting permission to use which SSH key." The modes are per application, per terminal tab, or per request. Its own warning on forwarding: "any new connection made within the remote environment using the same key is also authorized… for the duration of the session." The prompt names the process, not the destination host.
- **Secretive** keeps keys in the Secure Enclave, so they can't be exported. It can require Touch ID and "notifies you whenever your keys are accessed".
- **OpenSSH 8.9 or later: agent restrictions.**
  - `ssh-add -h` limits a key to named destinations.
  - The client sends `session-bind@openssh.com`, which carries "hostkey, session identifier, signature, is_forwarding". In that message, 'signature' is "the server's signature of the session identifier using the private hostkey, as sent in the final SSH2_MSG_KEXDH_REPLY/KEXECDH_REPLY". The agent "will verify the signature… refusing to accept a duplicate session identifier".
  - Limits: "any forwarding path is only as strong as its weakest link". Destination-restricted keys "can't currently be used for other signing" (ssh-keygen, CA operations). Clients that don't take part in binding confuse the agent.
- **Claude Code on the web:** "git credentials and signing keys stay outside the sandbox". Self-hosted runners sign commits "via a runner-managed shim that signs each commit via Anthropic's signing service". This is signing done as a service outside the VM.
- **Boundary (HashiCorp):** SSH targets "use injected application credentials to authenticate an SSH session between the client and end host… never being in the possession of a valid credential". The injected credential can be a password, a private key or a Vault SSH certificate. Session recording requires that injection mode.
- **Teleport** issues short-lived certificates: `tbot` defaults to 5-minute certificates, renewed about every minute, and writes an identity file plus an `ssh_config` for OpenSSH.
- **Tailscale SSH** needs no SSH keys: it authenticates "over WireGuard, using Tailscale node keys", Tailscale distributes host keys, and check mode adds re-authentication. The destination must run Tailscale.

**The abuse to design against: the agent as a signing oracle.** Assume the guest is hostile. It can ask a forwarded agent to sign anything. The dangerous case is a relay, not a key theft:
1. An attacker outside opens an SSH session to `github.com`.
2. They pass its session id and GitHub's real host-key signature into the VM over any allowed channel.
3. The guest asks the agent to sign userauth for that session.
4. The guest sends the signature back out, and the attacker is logged in as the user from outside.

`session-bind` alone does not stop this, because the binding really was signed by GitHub's host key.

**Silo-specific fix:** the boundary carries the VM's TCP connection to port 22, and SSH sends `KEX_ECDH_REPLY` (host key plus signature over `H`) in cleartext. The boundary can record which signatures appeared on connections the VM really opened. The agent then accepts a `session-bind` only when:
- the signature is one seen on a live VM connection, and
- the host key is in the boundary's own known_hosts (never the guest's).

That defeats the relay. Unbound sign requests are refused outright.

| Path | What the agent program inside sees | Cost and what breaks | Exposure of the real key | Proven by |
|---|---|---|---|---|
| **A. Rewrite git remotes to HTTPS** (`git config --system url."https://github.com/".insteadOf git@github.com:` and the `ssh://git@github.com/` form) | Ordinary git; the existing HTTPS token swap applies | Almost free. Doesn't cover `ssh` to servers, SSH commit signing, hosts without HTTPS, or tools that bypass git config (go-git, JGit) | None for SSH; same exposure as the HTTPS token | Claude Code sandbox docs ("switch the remote to HTTPS"); Claude Code web GitHub proxy |
| **B. Agent socket on the boundary over vsock** (`SSH_AUTH_SOCK` points at a socket relayed to a Silo agent on the Mac; keys come from Keychain, Secretive or 1Password behind it) | Normal `ssh-add -l`, ssh, git and `ssh-keygen -Y sign` | Medium: implement the agent protocol and its policy. Policy: (1) require `session-bind` and match it to KEX signatures the boundary saw; (2) host key from boundary-side known_hosts; (3) per-key allowed hosts; (4) parse the data to sign: userauth only for bound sessions, SSHSIG only for namespace `git`; (5) optional approval on first use per host per session. Breaks: clients that don't send `session-bind` (go `x/crypto/ssh`, libssh2, Paramiko, JGit, OpenSSH before 8.9) are refused, or fall back to a per-use prompt; ssh in the VM with `-A` onward | Never leaves the Mac. Within policy the guest can still use the key for anything it allows on an allowed host (any repo the key can push) | Docker sbx (forwarding, no policy); 1Password, Secretive (approval); OpenSSH agent-restrict (binding) |
| **C. Boundary as SSH man-in-the-middle proxy** (terminates the guest's SSH with host certificates from a Silo SSH CA trusted through `@cert-authority * <silo-ca>` in `/etc/ssh/ssh_known_hosts`; dials the real host with the real key and checks the real host key on the Mac) | ssh "works"; host key verification passes through the CA | High: an SSH server and client that relays channels. Breaks or needs work: agent forwarding onward, port forwarding, ProxyJump chains, SFTP subsystems, GSSAPI; a user known_hosts pinning the real key may conflict. **Gains:** the boundary sees `git-upload-pack`/`git-receive-pack '<repo>'` and the pkt-line ref updates, so it can block pushes, deletions and tags, or limit repos, as Claude Code web does for HTTPS | Never in the VM; the boundary holds it | Boundary SSH injection; Teleport proxy; Claude Code web GitHub proxy (HTTPS equivalent) |
| **D. Short-lived SSH certificates from a CA outside** (boundary mints a 5-minute certificate for a key generated in the VM) | A key plus certificate in `~/.ssh`; plain OpenSSH | Medium. Only works where the user controls `TrustedUserCAKeys` (their own servers); GitHub accepts SSH CAs only for Enterprise Cloud orgs, so not for most users | A key in the VM, but the certificate expires in minutes; principals and `force-command` limit it | Teleport Machine ID; Vault SSH; Boundary's Vault certificates |

**Recommendation: A by default for git hosts, then B for everything else (ssh to servers, SSH commit signing, git hosts without HTTPS).**
- A costs nothing, and git then inherits the push limits the HTTPS path already has.
- B is the only path that keeps the key on the Mac while leaving every OpenSSH client unmodified. Matching `session-bind` to the KEX signatures the boundary saw is something no product here does, because none of them owns the VM's network. It is the defence against the relay oracle.
- Keep C as the later step if the owner wants per-repo or no-push policy for git over SSH.
- Pre-fill GitHub, GitLab and Bitbucket host keys in the VM. The boundary also checks host keys on its side.

**Sources**
- https://code.claude.com/docs/en/sandboxing
- https://code.claude.com/docs/en/self-hosted-environments-deploy
- https://code.claude.com/docs/en/cloud-environments (GitHub proxy)
- https://docs.docker.com/ai/sandboxes/configuration/credentials/
- https://www.1password.dev/ssh/agent/security/
- https://www.1password.dev/ssh/agent/forwarding.md
- https://github.com/maxgoedjen/secretive
- https://www.openssh.org/agent-restrict.html
- https://raw.githubusercontent.com/openssh/openssh-portable/master/PROTOCOL.agent
- https://www.openssh.com/txt/release-8.9
- https://developer.hashicorp.com/boundary/docs/targets/create/ssh
- https://goteleport.com/docs/machine-workload-identity/machine-id/getting-started
- https://tailscale.com/kb/1193/tailscale-ssh

---

## 2. Tokens obtained by exchange (OAuth refresh, client credentials, STS-like)

**The problem.** The secret goes out (a refresh token, or a client secret in a form body), and that part already works. The new access token comes back in the response body into the VM.

**Prior art**
- **Managed Agents vaults.**
  - For their own MCP OAuth credentials (`mcp_oauth`), the refresh is done by the platform: "If you supply a `refresh` block, Anthropic refreshes the access token on your behalf."
  - For environment variables, substitution is outbound only, so the exchanged token reaches the sandbox. Their advice is to do the exchange outside.
  - Each credential is limited by `injection_location` (`header` / `body`) and `networking.allowed_hosts`.
- **Docker Sandboxes kit spec, `credential@1`.** A runtime "MUST intercept the OAuth `tokenEndpoint` and serve sentinel tokens, refreshing host-side; `passthrough: true` is the explicit opt-out and a security downgrade". The kit fields are:
  - `oauth.tokenEndpoint {host, path}`
  - `resourceHosts`
  - `sentinels {accessToken, refreshToken}`
  - `credentialFile` (writes the sentinels into the file the agent reads, for example `~/.claude/.credentials.json`)
  - `responseFields` (for providers with non-standard field names)
- **Fly Sprites Connectors.** "The Sprite never holds the provider token." The sprite calls `https://api.sprites.dev/v1/gateway/<provider>/<connection_id>/<path>`. The gateway knows which sprite is calling from "Fly.io's request signature" and attaches and refreshes credentials. This is a gateway URL, not transparent interception.
- **E2B workload identity** (private beta). The sandbox gets JWT-SVID tokens from `https://id.e2b.dev` and exchanges them itself, for example with `sts:AssumeRoleWithWebIdentity`. Their docs admit the resulting AWS credentials stay valid after the token expires or the sandbox ends, so E2B accepts exposure.
- **Codex cloud.** "Programs receive a placeholder; the proxy substitutes the real value… HTTPS on port 443". OIDC "lets a task obtain short-lived credentials" and the short-lived credential enters the task.

| Path | What the agent program inside sees | Cost and what breaks | Exposure | Proven by |
|---|---|---|---|---|
| **A. Intercept the token endpoint, return sentinels** (boundary rewrites the token-endpoint response, keeps sentinel→real for `resourceHosts`, and refreshes on the Mac; an incoming sentinel refresh token is swapped for the real one, or answered by the boundary itself) | A normal token response with fake `access_token`/`refresh_token` and the real `expires_in` | Medium: response-body rewriting (JSON, plus form-encoded responses such as GitHub's legacy format), per-provider descriptors (endpoint, field names), refresh-token rotation (store the new real token, keep the sentinel stable). If the access token is a JWT the client decodes (Azure, some IdPs), the sentinel must be a structurally valid fake JWT with the real claims, as Claude Code's `decode: "jwt"` / `maskClaims` produces. Breaks: clients that check the JWT signature locally (rare), and DPoP or mTLS-bound tokens | None in the VM | Docker sbx kit spec (normative); Docker OAuth for Claude Code/Codex |
| **B. Exchange outside, store the result as an as-is secret** | A placeholder access token; the client never refreshes | Low. Breaks when the token expires mid-session unless the boundary refreshes it; clients that insist on running their own refresh flow get confused | None | Managed Agents (recommended pattern; `mcp_oauth` refresh) |
| **C. Identity token plus a gateway URL** | The VM calls a Silo URL instead of the provider | Low to medium, but tools must be pointed at the gateway, so unmodified CLIs don't fit | None | Fly Connectors |
| **D. Let the short-lived result in** (the VM does the exchange with an identity Silo issues) | Real short-lived tokens | Low | The token is live in the VM until it expires (an E2B-style caveat: AWS credentials outlive the identity token) | E2B, Codex OIDC |

**Recommendation: A, with B as its degenerate case.** Silo already terminates TLS for allowed hosts, so rewriting the response of a known token endpoint is the same machinery run in reverse. Docker made it a MUST in a published spec, so it is a known, workable design. Ship descriptors for the endpoints that matter first: `platform.claude.com/v1/oauth/token`, `auth.openai.com`, `oauth2.googleapis.com/token`, `login.microsoftonline.com/*/oauth2/v2.0/token`, `github.com/login/oauth/access_token`. An unknown token endpoint falls back to D, and the Workshop should show that fallback as a visible downgrade.

**Sources**
- https://platform.claude.com/docs/en/managed-agents/vaults
- https://docs.docker.com/ai/sandboxes/configuration/credentials/
- https://github.com/docker/sandbox-kit-spec/blob/main/docs/spec/capabilities/com.docker.sandbox/credential@1.md
- https://docs.fly.io/sprites/concepts/connectors
- https://docs.e2b.dev/iam/workload-identity
- https://learn.chatgpt.com/docs/environments/cloud-environments
- https://code.claude.com/docs/en/settings-reference (decode/maskClaims)

---

## 3. GCP and Azure credentials

**Why these don't fit the as-is swap**
- GCP service-account JSON keys sign an RS256 JWT locally. google-auth either sends it to `oauth2.googleapis.com/token` (jwt-bearer grant) or uses a self-signed JWT directly as the bearer.
- gcloud user logins (`authorized_user` ADC) are refresh-token exchanges (kind 2).
- Azure certificate service principals sign a client assertion locally. Secret service principals send `client_secret` as-is in the body, but get a JWT access token back.

**The key move: the boundary can be the cloud metadata server.**
- It owns the VM's network, so it can answer `169.254.169.254` and `metadata.google.internal` exactly as a cloud VM does, with no environment variables at all.
- The paths don't collide: AWS uses `/latest/...`, GCP uses `/computeMetadata/v1/...` with `Metadata-Flavor: Google`, and Azure IMDS uses `/metadata/identity/oauth2/token` with `Metadata: true`.
- Prior art for emulation:
  - **salrashid123/gce_metadata_server** serves `access_token` and `identity?audience=` from a service-account key, impersonation, federation or a TPM. Its README documents it working with the Python, Java, Go, Node and .NET libraries and with `gcloud`.
  - **CoreWeave sandboxes** set `AWS_CONTAINER_CREDENTIALS_FULL_URI` plus `AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE`, and "the SDK refreshes the credentials before they expire".
  - **Azure App Service:** `GET {IDENTITY_ENDPOINT}?resource=…&api-version=2019-08-01` with `X-IDENTITY-HEADER`, returning `{access_token, expires_on, resource, token_type, client_id}`.
- **Then combine it with sentinels:** the metadata server returns a fake access token, and the boundary swaps in the real one on `*.googleapis.com` / `*.azure.com` / `management.azure.com`. The real token then never enters the VM either.
  - GCP `ya29.` tokens are opaque, so a sentinel is easy.
  - Azure tokens are JWTs that `az` and some libraries decode for tenant and oid, so they need a fake JWT with the real claims.

| Path | What the agent program inside sees | Cost and what breaks | Exposure | Proven by |
|---|---|---|---|---|
| **A. Metadata emulation plus sentinel token swap** | A "GCE VM / Azure VM": SDKs and `gcloud`/`az login --identity` pick up the identity automatically | Medium. Several details must be right:<br>• Answer DNS for `metadata.google.internal` and return the `Metadata-Flavor: Google` response header.<br>• google-auth-python also pings `GCE_METADATA_IP`.<br>• `gcloud` reads `GCE_METADATA_ROOT`; libraries read `GCE_METADATA_HOST`; neither is needed if you own the link-local IP.<br>• No `GOOGLE_APPLICATION_CREDENTIALS` or `~/.config/gcloud/application_default_credentials.json` may exist in the VM, because they come before metadata in ADC order.<br>• Serve `project-id` and `identity?audience=` (ID tokens for Cloud Run).<br>• gRPC APIs (Pub/Sub, Spanner, Firestore) need header substitution in HTTP/2.<br>• GCS signed URLs go through IAM `signBlob`, which works.<br>• Azure: IMDS answers with `Metadata: true`, or set `IDENTITY_ENDPOINT`/`IDENTITY_HEADER` (msal-node requires both). | None in the VM | gce_metadata_server; CoreWeave/AWS container credentials; Azure App Service MSI |
| **B. Metadata emulation that returns real short-lived tokens** | Same as A | Lower: no HTTP/2 or JWT faking | A one-hour token in the VM. GCP can scope it down: impersonate a narrower service account, or use Credential Access Boundaries for GCS | Same, as designed by the clouds |
| **C. Re-sign the JWT assertion at the boundary** (fake SA key in the VM; the boundary re-signs RS256 assertions to `oauth2.googleapis.com/token` and self-signed JWT bearers) | A normal SA JSON key file | High and fragile: every JWT form, self-signed JWTs on arbitrary `*.googleapis.com` hosts, Azure certificate assertions | None, if it works | Analogue of SigV4 re-signing; no product found |
| **D. Workload identity federation** (`external_account` with `credential_source.url` → a boundary OIDC issuer; executable sources need `GOOGLE_EXTERNAL_ACCOUNT_ALLOW_EXECUTABLES=1`) | A config file; the SDK exchanges at `sts.googleapis.com` | The user must create a WIF pool trusting Silo's issuer. The exchanged token comes back into the VM unless path 2A catches `sts.googleapis.com` | A short-lived token in the VM | Codex workload identity; E2B |

**Recommendation: A, starting with B (real tokens, no swap) as the first working cut.**
- The emulated metadata server is what every Google and Azure SDK already looks for, with zero configuration in the VM.
- The real long-lived credential (an SA key, the user's `gcloud` login, an `az` service principal) stays in the Mac keychain, and the boundary mints access tokens from it.
- Adding the sentinel swap later removes even the one-hour token. That needs the same HTTP/2 and fake-JWT work kind 2 needs anyway.
- One emulator at `169.254.169.254` can serve AWS IMDSv2 as well. This gives the AWS SDKs a path for presigned URLs and streaming uploads, which the re-signing path refuses (if the VM holds short-lived real credentials).

**Sources**
- https://github.com/salrashid123/gce_metadata_server
- https://googleapis.dev/python/google-auth/latest/reference/google.auth.environment_vars.html
- https://docs.cloud.google.com/iam/docs/workload-identity-federation-with-other-providers
- https://learn.microsoft.com/en-us/azure/app-service/overview-managed-identity
- https://cdn.jsdelivr.net/npm/@azure/msal-node@5.2.2/src/client/ManagedIdentitySources/AppService.ts
- https://docs.coreweave.com/products/sandboxes/operations/object-storage-access
- https://learn.chatgpt.com/docs/enterprise/workload-identity
- https://docs.e2b.dev/iam/workload-identity

---

## 4. MySQL (`caching_sha2_password`), brief

**Protocol.** This is MySQL's default plugin ("In MySQL 8.4, `caching_sha2_password` is the default").
- **Fast path:** a SHA-256 challenge-response over the server's 20-byte nonce. "A scramble is used… to prevent repeat attacks", so no substitution in place is possible.
- **Full authentication** (cache miss, after a password change or `FLUSH PRIVILEGES`): sends the cleartext password, which requires TLS or an RSA key exchange ("ERROR 2061… Authentication requires secure connection").
- **TLS** starts mid-stream: an SSL request packet follows the server greeting. The `mysql` client trusts `--ssl-ca`, not the system store.

| Path | Inside | Cost and what breaks | Exposure | Proven by |
|---|---|---|---|---|
| **A. Protocol-aware proxy with two separate logins** (the boundary plays the server to the VM with its own nonce and accepts the placeholder password; it plays the client to the real server with the real password and does TLS, RSA or full auth upstream) | `DATABASE_URL` with a placeholder password; any driver | Medium; same shape as the Postgres SCRAM swap already built. TLS from the VM: either `ssl-mode=PREFERRED/DISABLED` to the boundary (it is local) with TLS upstream, or terminate with Silo's CA (only if `--ssl-ca` points at it, because `VERIFY_IDENTITY` uses the client's own CA file). Breaks: `COM_CHANGE_USER` to another user; `mysql_clear_password`/LDAP plugins need their own handling | None | ProxySQL (separate frontend and backend authentication; `caching_sha2_password` on both sides since 2.6.x); Teleport DB access |
| **B. Real password in the VM, host allowlist only** | The real secret | Free | Full | n/a |

**Recommendation: A.** Reuse the Postgres proxy's structure. Always use TLS upstream so full authentication works on a cache miss.

**Sources**
- https://dev.mysql.com/doc/refman/8.4/en/caching-sha2-pluggable-authentication.html
- https://proxysql.com/documentation/authentication-methods
- https://www.percona.com/blog/caching_sha2_password-support-for-proxysql-is-finally-available/

---

## Five things to build first, for the most value

- **The git `insteadOf` rewrite to HTTPS, set in the VM's system git config.** It sends git-over-SSH traffic through the existing HTTPS swap and push limits at almost no cost, and it covers most of what coding agents do over SSH.
- **A boundary-held SSH agent over vsock that refuses unbound sign requests.** It accepts a `session-bind` only when the signature is one the boundary saw on a real VM connection's KEX, checks the host key against boundary-side known_hosts, keeps per-key host lists, and signs SSHSIG only for namespace `git`. It keeps keys on the Mac, works with stock OpenSSH, and closes the relay oracle that plain forwarding (Docker, 1Password) leaves open. I found no product doing this match, so it is a unique claim for Silo.
- **A token-endpoint interceptor driven by descriptors** (endpoint, field names, sentinels, credential file), following Docker's `credential@1` model. It fixes "the exchanged token comes back into the VM" for Claude/Codex logins, Google and Microsoft OAuth, and GitHub OAuth apps.
- **One link-local metadata emulator for GCP, Azure IMDS and AWS IMDSv2, minting from keychain-held credentials.** Every cloud SDK and CLI then authenticates with no configuration in the VM. It starts with real one-hour tokens; sentinels can be added once the interceptor handles JWTs and HTTP/2.
- **A MySQL handshake proxy built on the Postgres SCRAM proxy design.** It covers the other common database with a known structure, and makes the database story complete for the Workshop choice.

**A decision to raise with the owner.** Silo's current rule is "the VM holds a placeholder". Two of these recommendations (the first cut of the metadata emulator, and the fallback for unknown token endpoints) deliberately let a short-lived real token into the VM. The Workshop should state that downgrade per credential, because otherwise "secrets never enter the VM" stops being a true sentence. A short-lived real token is a real case the current wording leaves out.


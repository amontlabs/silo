# Existing credential brokers for Silo's E2B refactor

Date: 2026-09-24. Scope: primary documentation and source research. No candidate
was installed or executed, no credential store was read, and no VM was changed.
This selects the next qualification candidate, not a production dependency.

**Scope update, retaining MicroSandbox:** the user now wants to bring upstream
snapshots/checkpoints/forks into Silo. Retain MicroSandbox's built-in credential
proxy as the baseline. Silo already uses its placeholder/TLS substitution and
adds host-store custody, assignment policy, GitHub integration, selective
interception and connection revocation through application code and runtime
patches. The [0.7.2 secret contract](https://github.com/superradcompany/microsandbox/blob/v0.7.2/docs/sandboxes/secrets.mdx)
still provides host-side credential substitution. Reconcile those patches and
qualify current grants across restore/fork during the runtime upgrade. No
security or maintenance benefit from replacing this boundary with iron-proxy
has been demonstrated. The recommendation below concerns replacing the E2B
fixture broker; it is not a recommendation to add a broker to retained
MicroSandbox. Revisit it only for a concrete gap that upstream cannot resolve.

## Recommendation

Qualify **iron.sh's iron-proxy first**, with **Infisical's standalone Agent Vault**
as the second candidate. The user identified iron.sh after the initial comparison.
Its [standalone proxy](https://github.com/paradigmxyz/iron-proxy/blob/v0.50.0/README.md)
is a closer architectural fit: it supplies egress filtering and substitution
without requiring a new credential database or the hosted control plane. This is
an integration-cost assessment, not a claim that iron-proxy has the longest
production history or has passed an independent security audit.

Infisical remains relevant where broker-managed identities and sessions justify
the additional custody integration. Its publisher is an established secrets
vendor, but the component is recent; the project itself recommends its platform
offering for production/enterprise use. See the
[project description](https://github.com/Infisical/agent-vault/blob/v0.39.3/README.md)
and [platform overview](https://infisical.com/docs/documentation/platform/agent-vault/overview).

Do not ship our Python broker or build a replacement HTTP/TLS implementation.
If a candidate fails a required property, record the reproduction and pursue
supported upstream changes or a supported vendor deployment. Do not quietly
weaken Silo's credential contract to make a dependency fit.

No reviewed tool is yet demonstrated to meet the entire existing contract:
host credential-store custody, unmodified Git/LFS/gh, per-workspace grants,
immediate revocation, selective TLS interception and snapshot-safe authority.

## Requirements used for the comparison

The baseline is the [credential contract audit](e2b-credential-contract-2026-09-23.md),
[secrets contract](../SiloUI-SECRETS.md) and current
[`github_tokens.rs`](../../app/SiloUI/src-tauri/engine/src/github_tokens.rs).

- Run outside guest VMs on Silo's owned execution infrastructure, including
  Linux ARM64 and x86-64. A managed cloud dependency is not an assumed substitute.
- Keep provider tokens, parent OAuth credentials, CA private keys and management
  authority outside guest memory, disks and snapshots.
- Support ordinary HTTPS clients, Git smart HTTP, LFS, REST/GraphQL and generic
  secrets without replacing the user's tools with a proprietary tool API.
- Bind requests to the current workspace and runtime incarnation. A guest must
  not obtain another workspace's authority by choosing a header or proxy port.
- Apply rotations/removals to existing connections and use current policy after
  restore/fork. Owner loss and stale policy cannot silently retain authority.
- Preserve GitHub's exact Authorization substitution boundary and the explicit
  generic-secret assignment rules. Keep ordinary unrelated TLS traffic working.
- Ship a maintained, identifiable release with a security process and compatible
  distribution terms. Count the remaining custom integration, not just features.

## Candidate comparison

| Candidate | What is already supplied | Fit and disposition |
| --- | --- | --- |
| **iron.sh / iron-proxy** | Standalone egress proxy, placeholder replacement, configurable header scope, HTTP CONNECT/SOCKS5 and external secret sources. [Release README](https://github.com/paradigmxyz/iron-proxy/blob/v0.50.0/README.md) | **First qualification candidate.** Smaller custody footprint. Workspace identity, exact matching, revocation and E2B network attachment remain unqualified. |
| **Infisical Agent Vault, standalone** | Independent broker binary, HTTPS CONNECT interception, service rules, placeholder/header injection, agent/session identity. [README](https://github.com/Infisical/agent-vault/blob/v0.39.3/README.md) | **Second qualification candidate.** Local deployment fits; OS credential-store integration, revocation and selective interception still need proof. |
| **Infisical platform Agent Vault / Agent Proxy** | Credential brokering integrated with Infisical secrets and access management. [Agent Proxy](https://infisical.com/docs/documentation/platform/agent-proxy/overview), [Agent Vault](https://infisical.com/docs/documentation/platform/agent-vault/overview) | Vendor-supported alternative with a larger service/storage footprint. Confirm self-hosted feature entitlement and redistribution; do not add an account/service requirement implicitly. |
| **CyberArk Secretless Broker** | Established standalone broker and configurable HTTP credential headers. [Generic connector](https://github.com/cyberark/secretless-broker/blob/main/internal/plugin/connectors/http/generic/README.md) | **Not a drop-in for Silo's HTTPS clients:** its HTTP proxy rejects CONNECT. It also deliberately has no client authentication. Useful prior art, not the primary candidate. |
| **mitmproxy / mitmdump** | Maintained HTTPS interception, certificate handling, HTTP/2, streaming and header modification. [Features](https://docs.mitmproxy.org/stable/overview/features/), [protocols](https://docs.mitmproxy.org/stable/concepts/protocols/) | Reusable proxy engine if complete brokers fail, but Silo would still own credential authorization, secret-store integration and lifecycle policy in an addon. That remaining security code needs explicit justification. |
| **Envoy credential injector** | Built-in outgoing HTTP credential injection. [Filter](https://www.envoyproxy.io/docs/envoy/latest/configuration/http/http_filters/credential_injector_filter) | **Exclude this filter for the proposed boundary:** its documented security posture assumes trusted downstream and upstream peers. This conclusion concerns the filter, not every Envoy deployment. |
| **E2B's supplied secret injection** | Runtime markers resolved outside the guest by its enterprise orchestrator. [Pinned architecture](https://github.com/e2b-dev/runtime/blob/a065a4ddb3f2c6a4149634d9acb14b62f65839ac/docs/ARCHITECTURE.md) | Supported upstream option if available for our deployment. The pinned Embed stack does not configure the required backend, and its open-source proxy does not support BYOP. Local redistribution/support remains unestablished. |
| **Docker Sandboxes** | Host-side token injection, sandbox scopes and experimental custom-secret substitution. [Credentials](https://docs.docker.com/ai/sandboxes/configuration/credentials/) | A complete alternative sandbox product. The reviewed interface is not a separately deployable E2B credential broker. Replacing the runtime is a separate decision. |
| **GitHub Agentic Workflow Firewall** | Docker/Squid isolation plus an API credential sidecar for supported model providers. [Usage](https://github.com/github/gh-aw-firewall/blob/main/docs/usage.md) | Recognized upstream prior art; its documented sidecar is not a general replacement for Silo's GitHub and arbitrary-secret contract. |
| **HashiCorp Vault Agent templates** | Established secret retrieval and rendering to application files. [Templates](https://developer.hashicorp.com/vault/docs/agent-and-proxy/agent/template) | Secret custody/delivery is a different layer. Rendering credentials inside the guest does not satisfy this task; a broker is still required. |

## Findings that affect adoption

### iron.sh: smaller proxy component, with exact compatibility gates

The original `ironsh/iron-proxy` URL redirects to `paradigmxyz/iron-proxy`.
The inspected [v0.50.0 release](https://github.com/paradigmxyz/iron-proxy/releases/tag/v0.50.0)
is dated September 18, 2026. The repository identifies an Apache-2.0 license;
the [security policy](https://github.com/paradigmxyz/iron-proxy/blob/v0.50.0/SECURITY.md)
provides private vulnerability reporting. These facts establish a distributable
upstream candidate and maintenance activity, not production security acceptance.

Its [CONNECT/SOCKS5 listener](https://docs.iron.sh/guides/socks5-connect) supports
HTTP/TLS clients; it is not a general SSH/raw-TCP relay. The
[v0.50.0 substitution code](https://github.com/paradigmxyz/iron-proxy/blob/v0.50.0/internal/transform/secrets/secrets.go)
decodes and re-encodes Basic Authorization, which is relevant to Git. However,
it performs substring replacement, not Silo's complete Authorization-value
comparison. `require: true` alone does not close that semantic gap. Qualify the
exact Bearer/Basic cases and pursue a narrow upstream option if needed.

The [static-secret documentation](https://docs.iron.sh/credential-proxying/static-secrets)
supports named-header replacement and fixed host/method/path rules. For GitHub,
restrict scanning to Authorization and disable body, query and path scanning.
Do not treat injection at every matching host as equivalent to exact substitution.
An upstream server still receives the token and can reflect it.

The [release README](https://github.com/paradigmxyz/iron-proxy/blob/v0.50.0/README.md#secrets)
documents environment, file and external-store sources. Environment values are
fixed at startup; files can refresh on reload or TTL. Silo can investigate
supplying host-store values directly to the broker process environment, without
writing an `.env` file. Rotation would then restart the broker, not the guest.
This integration and its interruption behavior remain untested. The management
API atomically swaps config pipelines, but that is not evidence it revokes
already-authorized streams. Test connection cancellation separately.

Use the external-network placement from the
[deployment comparison](https://docs.iron.sh/deploy/overview). Several examples
run the proxy inside the workload VM and rely on non-root workloads; that is
incompatible with Silo's guest-root threat model and memory snapshots. Our proxy,
CA key and current policy must remain outside each guest. Host network controls
must bind each guest to its current grant and stop cross-workspace access; a
copied placeholder cannot serve as sufficient workspace identity. No E2B
attachment, live-revocation or snapshot-replay pass is claimed.

### Infisical: identity/session features, with custody gaps

The observed [latest release](https://github.com/Infisical/agent-vault/releases/tag/v0.39.3)
was **v0.39.3**, published September 1, 2026. The release provides checksum and
signature-verification instructions. The [license](https://github.com/Infisical/agent-vault/blob/main/LICENSE)
uses MIT terms outside any `ee/` exceptions and third-party components; verify the
actual distributed artifact's notices. Its [security policy](https://github.com/Infisical/agent-vault/blob/main/SECURITY.md)
has a vulnerability-reporting channel and says fixes ship in the latest release.
These are maintenance signals, not an independent audit result.

Important deployment facts from the [security documentation](https://docs.agent-vault.dev/learn/security):
the broker accepts scoped sessions, while long-lived agent tokens have no expiry;
its forward-proxy authentication travels over plaintext HTTP to the broker;
and passwordless storage leaves the database encryption key unwrapped. Silo must
use an isolated or encrypted transport, avoid long-lived guest authority and
keep management access outside the guest. The CA private key also belongs there.

Its [permissions model](https://docs.agent-vault.dev/learn/permissions) supplies
restricted proxy access. Use a guest principal with no instance management role
and only the intended vault's proxy permission. Do not give the guest a member
or admin session. A broker capability is still usable authority, even though it
cannot authenticate directly to GitHub: snapshots and copied capabilities must
not bypass Silo's current workspace policy.

The documented [credential stores](https://github.com/Infisical/agent-vault/blob/main/docs/learn/credential-stores.mdx)
are local encrypted storage or Infisical. There is no documented Keychain/Secret
Service backend in that list. External-store synchronization serves the previous
snapshot during outages, with no automatic fail-closed deadline. Therefore,
deploying that mode unchanged does not implement Silo's owner-loss/revocation
contract. Copying long-lived host secrets into another database is not an
approved workaround. Qualify a supported custody integration and explicit policy
invalidation before accepting this dependency.

[Service rules](https://docs.agent-vault.dev/learn/services) distinguish typed
header injection from configurable substitutions. Unmatched traffic passes by
default; explicit denial is available. Header substitution scans all headers,
whereas Silo's GitHub path requires an exact Authorization value. Unconditionally
overwriting Authorization also differs from that contract. Neither is evidence
of equivalent semantics. Check selective TLS bypass, host/port rules, existing
connections and Git/LFS redirects on the selected release. Main-branch docs can
describe behavior newer than the release; pin and verify before execution.

### CyberArk: concrete transport and isolation blockers

The [HTTP proxy implementation](https://github.com/cyberark/secretless-broker/blob/main/internal/plugin/connectors/http/proxy_service.go)
returns 405 for CONNECT. It therefore cannot simply replace `HTTPS_PROXY` for
Silo's existing HTTPS clients. Its [hardening guidance](https://github.com/cyberark/secretless-broker/blob/main/README.md#security-hardening)
also states that any client reaching a listener inherits its backend access:
network isolation supplies the client boundary.

Version selection matters: the inspected [changelog](https://github.com/cyberark/secretless-broker/blob/main/CHANGELOG.md)
records upstream destination pinning in 1.7.34 and denial/TLS-default changes in
2.0.0. The observed GitHub [latest-release endpoint](https://github.com/cyberark/secretless-broker/releases/latest)
resolved to v1.7.32. That is not proof those later fixes are absent from all
published images; it is a release-channel discrepancy that must be resolved.
Do not recommend an older binary using guarantees read from `main`.

### Envoy and E2B: component availability is not enough

Envoy's [credential-injector API documentation](https://www.envoyproxy.io/docs/envoy/latest/api-v3/extensions/filters/http/credential_injector/v3/credential_injector.proto)
says it lacks substantial production burn time and assumes both peers are
trusted. An untrusted guest violates that assumption.

E2B's [open-source TCP proxy](https://github.com/e2b-dev/runtime/blob/a065a4ddb3f2c6a4149634d9acb14b62f65839ac/packages/orchestrator/pkg/tcpfirewall/proxy.go)
returns false from `SupportsBYOP`. An SDK field does not establish a supported
host-network attachment. Ask E2B for the local/embedded injection deployment and
distribution contract if pursuing that option. No vendor contact occurred in
this research.

## What Silo should still own

Keep workspace assignment, the user's permission decisions and runtime lifecycle
binding. Reuse a broker for HTTP/TLS transport and credential application; reuse
the OS credential store for custody unless an explicit replacement is selected.
Keep policy outside guest snapshots and keep broker management host-only.

For GitHub, continue using GitHub-issued restricted credentials. Silo already
calls the documented [scoped user-token endpoint](https://docs.github.com/en/rest/apps/apps#create-a-scoped-access-token),
which restricts repositories and permissions. Do not replace that boundary with
our fixture's HTTP-path or GraphQL authorization parser. Choosing the appropriate
grant for a request still needs integration, especially for mixed read/write
grants and GraphQL. PAT mode retains its distinct full-token authority; proxying
a broad PAT does not turn it into a repository-scoped token.

Network enforcement is also separate from proxy configuration. `HTTPS_PROXY`
helps cooperative clients route traffic; guest root can ignore it. Qualify the
execution host's network controls and workspace-to-broker identity mapping.
No broker can guarantee secrecy from a trusted destination that reflects a token,
or stop every harmful operation the granted provider token itself permits.

## Next qualification experiment

Use a pinned upstream iron-proxy release first, then Agent Vault if required,
on a disposable host-side deployment with two scratch guests, synthetic
credentials and an independent recording origin.
Keep the existing Python broker only as historical fixture evidence. Required
observations:

1. The allowed request reaches the origin with the expected token digest;
   wrong origin, port, placeholder and cross-workspace identity attempts have
   zero unauthorized receipts. The guest cannot reach broker administration.
2. Existing Git, LFS and gh clients work through HTTPS CONNECT, with exact
   Authorization semantics, bounded streaming and TLS verification. Unrelated
   TLS remains unintercepted; redirects cannot move an injected credential to
   another origin. Test generic secrets separately.
3. Rotation/removal invalidates both new requests and established connections.
   Owner loss, broker restart and policy-store failure deny stale authority.
   Capture the revocation boundary rather than counting a transport error alone.
4. Restore/fork cannot reuse old authority or another workspace's capability.
   A positive control succeeds with the new current grant while the old grant
   has no receipt. Verify network binding, not merely a guest-supplied token.
5. Prove supported host-store custody without persistent extra copies of parent
   credentials; inspect configuration, logs and snapshot state for synthetic
   values. Then repeat the necessary provider cases with disposable scoped
   credentials, not the user's broad session token.

If these require rebuilding a proxy or policy engine inside Silo, this candidate
has failed the reuse goal. Record the missing feature and resolve it upstream
or evaluate the supported vendor alternative before implementation proceeds.

## Broader engineering rule

Apply the same selection process to the rest of the refactor: E2B's official SDK
for runtime operations, OpenSSH for SSH, the selected upstream viewer for remote
desktop, LCU for computer use, and existing credential-store libraries for secret
custody. Product glue is expected; duplicate protocol stacks and infrastructure
are not the default. Every proposed custom subsystem needs a cited comparison,
a concrete unmet requirement, and the smallest justified integration surface.

Validation in this research is limited to documentation/source inspection and
repository diff checks. No runtime security, performance, packaging or provider
qualification pass is claimed.

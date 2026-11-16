# README and website value review

Date: 2026-09-29. Scope: the current README, published website, rendered GitHub
README, website source, and linked product documentation. This review changes
neither the public copy nor the application.

## Position

Prioritize the path from a visitor's task to a first observable result. The
largest gap is the agent-desktop setup path: the website leads with agents,
while its feature tabs cover computers, SSH tools, and development servers.
The 59-second film does illustrate agent desktop use. It is useful product
explanation, but its fixture scenes do not establish live setup success.

The review establishes content and navigation gaps. It contains no reader
interviews, observed installation attempts, funnel measurements, or evidence
that these gaps cause abandonment. Do not report an expected conversion lift
or infer demand from visitors, stars, or interest.

## Evidence

- [Published website](https://silo.polarzero.xyz/): inspected in the in-app browser.
  Its hero says "Computers for your agents." The feature tabs describe remote
  computers, SSH-capable tools, and port forwarding. There is no agent-desktop
  setup section or matching feature tab.
- [Rendered README](https://github.com/0xpolarzero/silo#readme-ov-file): inspected
  through the website's View source link. It matches the local [README](../../README.md).
  The README names Codex, Claude Code, and Cursor, says users install and sign in
  inside the sandbox, and provides generic start-working steps. It does not give
  a complete agent task with an expected result.
- [Website source](../../website/index.html): "About GitHub access" links to
  `https://github.com/0xpolarzero/silo#github-secrets-and-backups`. That heading is
  absent from both the current local and rendered README. The link reaches the
  repository without locating the promised explanation.
- [Agent tools](../SiloUI-LUDA.md): desktop provisioning registers supported
  clients for the guest `silo` account. Agent installation, authentication,
  client approvals, and reconnection remain relevant. Host-side agents do not
  inherit guest MCP configuration merely by issuing SSH commands.
- [Secrets](../SiloUI-SECRETS.md): the document begins with credential-store,
  metadata, caching, and reconciliation details. Its Boundary section explains
  the important user limits: proxied HTTPS credentials, trusted allowed
  destinations, possible server reflection, and no local-signing-key support.
- [Documentation index](../README.md): opens with source layout and an
  implementation/operations table. It does not provide a task-based user-guide
  entry point.
- Tour transcript and
  [website README](../../website/README.md): agent sessions and external apps
  are illustrations; the interactive demo uses read-only sample data.
- The website specifies supported OS/architectures and KVM, but omits the
  README's Linux Secret Service requirement and the optional desktop download
  distinction. Neither surface provides measured, workload-specific host
  resource guidance. Do not invent a minimum RAM figure or setup duration.

The web retrieval tool returned an older GitHub README and failed to fetch the
website. The browser's current rendered pages resolved those discrepancies;
the older retrieval is not evidence of what readers currently see.

## Recommended order

1. **Repair the GitHub explanation link.** Point it at an existing, user-oriented
   explanation and preserve stable destinations when reorganizing the README.
2. **Publish one complete agent-desktop quickstart.** Specify guest versus host,
   what Silo provisions, what the reader installs/authenticates, how to start the
   desktop and agent, approvals, one harmless task, and the visible success
   condition. Validate on the exact downloadable release before publishing
   commands. Link it from the hero, feature section, and README.
3. **Make the hero explain the job.** Keep the agent headline if agents are the
   intended primary audience; expand the subheading to mention an isolated Linux
   desktop on the reader's own computers, compatible tools, and scoped access.
   Surface Apple Silicon/macOS and Linux compatibility, MIT licensing, and the
   existing tour beside the download decision.
4. **Give readers a choice of concrete tasks.** Use agent desktop, development
   server, and remote computer as entry points. Keep the full README quickstart
   for execution and concise previews on the website. Explain when an existing
   Docker or SSH workflow already suffices rather than asserting universal
   superiority or unsupported performance advantages.
5. **Create task-based user documentation.** Put setup, supported tools, remote
   routing requirements, access controls, recovery, and common failures before
   implementation research. Reuse existing technical documents for deeper
   explanations. Avoid moving or deleting useful research.
6. **Explain costs and boundaries at the decision point.** Distinguish bundled
   runtime from optional desktop downloads and separately installed agents;
   clarify agent/provider account costs without making provider pricing claims.
   Summarize repository/token differences, HTTPS secret restrictions, remote
   ownership, and file persistence. Provide measured resource guidance only
   after collecting workload-specific evidence.
7. **Connect the demo to an outcome.** Add task entry points or guided navigation
   to relevant sample screens and their real setup guides. Preserve the
   read-only/sample-data labels and do not imply the demo runs an agent or VM.
   Use a separate real-release recording as operational evidence if obtained.

## Proposed validation

Recruit five intended users who already run agents or manage development
environments. First ask about their last actual task, workaround, and failure,
not whether they like Silo. Observe them using the downloadable release and
quickstart without author coaching. A proposed acceptance gate is at least four
of five reaching the documented first result and correctly identifying where
the agent runs and which access they granted. Record task failures and time;
do not call this small test a statistical conversion estimate.

Start with the verified agent-desktop quickstart. Expanding screenshots, slogans,
or feature copy before observing that workflow risks polishing without users.

## Verification limits

This was a read-only website and source review using ordinary browser state and
the local repository. No native bundle, live VM, installation, or agent session
was tested. No app checks are needed for this research note; public content,
release configuration, versions, and deployment were unchanged.

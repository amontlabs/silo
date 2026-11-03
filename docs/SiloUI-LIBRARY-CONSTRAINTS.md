# macOS VM library-loading proof

Tested on 10 September 2026, macOS 26.5 (25F71) and macOS 14.8.7
(23J520), Apple Silicon, with System Integrity Protection enabled.

## Finding

Apple's library constraints work with ad-hoc signing on both tested systems. A hardened
`msb` can load the exact bundled `libkrunfw` identified by its code-directory hash
and reject other non-system libraries at load time. No Developer ID certificate
was used. This is OS enforcement, not a Silo checksum check before launch.

The standard library-validation exception is still present in the test executable,
but an embedded library constraint replaces unrestricted loading with an exact
code-hash rule. Apple explicitly describes this combination as a use for library
constraints. Hardened runtime remains enabled. Operating-system libraries are
exempt from library constraints.

## Observed behavior

| Test | Result |
| --- | --- |
| Standard validation with an ad-hoc library | Rejected for lack of a matching Apple Team ID. |
| Broad exception alone | Harmless substitute library executes, demonstrating the missing barrier. |
| Exception plus exact code-hash constraint | Approved library executes. |
| Different library with the same signing identifier | Rejected by the library constraint before its initializer executes. |
| Replacement at the approved pathname | Rejected; names and paths do not satisfy the hash constraint. |
| Approved library copied to another path | Accepted; the rule approves code, not a path. |
| Replaced indirect dependency | Rejected even though the directly loaded parent library is approved. |
| Unsigned substitute | Rejected. |
| Modified approved library retaining its old signature | Signature verification fails; macOS kills the test loader for an invalid code page before its initializer executes. |
| Loader copied and re-signed without its constraint | Substitute executes, demonstrating the whole-executable limitation. |

The real runtime test copied Silo's optimized `msb` and bundled engine into an
isolated temporary directory. With the exact engine hash constrained, it created
and booted a one-CPU, 512 MB computer from Silo's bundled image, executed a guest command,
and stopped. An alternate engine path was rejected with an explicit library-load
constraint error. Replacing the actual engine path with a different library using
the original signing identifier also failed. Restoring the approved engine allowed
the computer to boot and execute again. The disposable computer was stopped afterward.

The original Silo app, its running `dev` computer, and release signing settings
were not changed by these experiments. Replacement libraries only print a test
marker; they contain no malicious payload. Initializer markers use unbuffered
output so a process crash cannot conceal buffered output.

## Reproduce

From the repository root, with a built Apple Silicon release bundle:

```sh
python3 app/SiloUI/scripts/test-macos-library-constraints.py \
  --runtime-bundle app/SiloUI/src-tauri/target/release/bundle/macos/Silo.app \
  --guest-image app/SiloUI/src-tauri/runtime/guest-image \
  --output app/SiloUI/test-results/library-constraints-20260910
```

Use a new output directory for another run. Omit the two runtime arguments to
run only the library probes. The command requires macOS 14+, Python and Xcode
command-line tools; runtime testing also requires Apple Silicon virtualization.
It does not change system security settings or the supplied app bundle.

The recorded run passed all 11 library cases and the optional real-computer checks.
`results.json` and individual command logs are retained under that ignored output
directory. The unique computer home was stopped and removed. The associated initial
tamper experiment's crash report confirmed `CODESIGNING`, `Invalid Page`; raw
system crash reports are not copied into the repository. Tampering is reported
separately from a library-constraint rejection because page signature enforcement
can terminate the loader rather than return a `dlopen` error.

## Minimum-OS and final-package checks

All 11 library cases also passed in an isolated macOS Sonoma 14.8.7 VM with
System Integrity Protection and authenticated root enabled and no boot arguments.
The VM used Cirrus Labs’ vanilla Sonoma image, digest
`sha256:7ea0d508380b63f13c94ee72cd22ca3dd9ec5cdf4a5ae14b2ae2ce6a6603fba6`.
Evidence was retained locally at
`/private/tmp/silo-macos14-proof/silo-sonoma-constraints/results.json`; the VM
was shut down after testing.

GitHub-hosted macOS 14 and 15 runners had SIP disabled. They enforce the exact
library constraints but do not enforce the two signature controls tested here.
CI therefore runs nine constraint cases with those two controls explicitly
skipped, not reported as passed. The full suite on a normally protected Mac
remains a separate release acceptance check.

The final local 0.1.1 DMG was mounted read-only and passed bundle verification.
Its unmodified bundled helper and engine imported the bundled guest image,
created and started an isolated computer, executed a marker, and stopped successfully.
This checks the packaged runtime on macOS 26.5; it does not test Gatekeeper
first-launch behavior or the user’s update interaction.

## Limits and release requirements

- This establishes a library-load boundary while the constrained executable
  remains authentic. It does not authenticate a wholly replaced or redistributed
  app: anyone can ad-hoc sign a replacement executable with the rule removed.
  Silo's signed updater supplies a separate trusted delivery check.
- This does not detect compromised code that we intentionally approve and ship.
- macOS 14 introduced these APIs. Both the minimum supported major version and
  the current device passed the full suite. Do not silently relax the rule if an OS
  rejects it.
- Generate the approved hash from the final signed engine. Sign the helper with
  `--enforce-constraint-validity`; require the constraint, hardened runtime and
  exact hash in package verification. Any later re-signing must preserve or
  regenerate the constraint. Do not approve libraries by signing identifier alone.
- Release packaging now applies this rule to the final helper, verifies the exact
  policy using Apple’s signer, then creates and signs the updater archive and DMG.
- Optimized local macOS builds use the same finalization and verification through
  `npm --prefix app/SiloUI run desktop:build`. Tauri's generic app signature alone
  does not grant the VM helper permission to load its engine. Local builds create
  only the finalized `.app`; distribution archives remain a later release step.

## Primary sources

- [Apple: Protect your Mac app with environment constraints](https://developer.apple.com/videos/play/wwdc2023/10266/), especially library constraints and replacing unrestricted loading.
- [Apple: Defining launch environment and library constraints](https://developer.apple.com/documentation/security/defining-launch-environment-and-library-constraints).
- [Apple: Applying launch environment and library constraints](https://developer.apple.com/documentation/security/applying-launch-environment-and-library-constraints).
- The installed `codesign(1)` manual: `--library-constraint`, `--enforce-constraint-validity`, and ad-hoc signing without an identity.
- [MicroSandbox's existing entitlements](https://github.com/superradcompany/microsandbox/blob/main/msb-entitlements.plist). They establish its current setting, not that a broader exception is unavoidable.

# How does a computer use a key that signs each request

## The choice

An agent's part, 24 November 2026. An agent in main notarizes your Mac app with `notarytool`, and
uploads an iPhone build to TestFlight. Both read your App Store Connect key, a `.p8` file, and
sign a token with it inside the computer. The key itself is never sent, so the placeholder of
`secret-placeholder` cannot be swapped in: it is answered there as to find out.

The options:

- A placeholder key, re-signed at the boundary (recommended). The computer holds a key of the same
  kind that Silo made, with a placeholder key id. The tools sign with it as usual. The boundary,
  which already ends the encrypted connection to the hosts that get a secret, checks that the
  placeholder signed the token, that it is for App Store Connect and lives no more than 20 minutes,
  and signs the same claims again with your real key. On a `GET`, it adds Apple's `scope` claim
  for that one request. It costs a re-signer for each kind of key. And while the computer runs, an
  agent can still make any call the key's role allows, as with any swapped secret: give it a key
  whose role is no wider than the work.
- The real key in the computer. It costs the key: an agent can copy it, and with an Admin key
  revoke your certificates or add users from anywhere.
- These tools run on your Mac. It costs the agent's work: it stops at every release step.

Recommended: a placeholder key, re-signed at the boundary. Codex's network proxy already does this
for AWS request signatures, so it is a known practice, and it covers every tool that talks to App
Store Connect at once.

## Notes

An agent's research, 24 November 2026: `research/2026-11-24-mac-and-ios-developers/README.md`, section "App Store Connect,
TestFlight and notarization". The prototype in that folder, `prototype/asc_resign.py`, ran on
Linux: a tool's token through the boundary was accepted by a fake App Store Connect that trusts only
the real key, and the placeholder token sent straight to it, to another host, with a key of the
agent's own, with a lifetime of a day or for another audience was refused. It has no TLS; the
kit's test 21 covers the computer's trust of Silo's certificate. Mac test 28 would run the real
tools through it with a real key kept on the Mac.

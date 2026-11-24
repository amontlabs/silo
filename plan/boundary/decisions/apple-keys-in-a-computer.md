# Which of your Apple keys may enter a computer

## The choice

An agent's part, 24 November 2026. You make an iPhone app and a Mac app sold outside the App Store.
You have a development certificate, an App Store distribution certificate, a Developer ID
certificate and an App Store Connect key. An agent in main builds, tests, sends builds to TestFlight
and prepares releases.

The options:

- None of your account's keys (recommended). Simulator builds need none. A development certificate
  that Xcode makes in the computer is that computer's own, and you can revoke it when the computer goes. App
  Store builds use Apple's cloud-managed certificate through the App Store Connect key, re-signed
  at the boundary. A Developer ID signing happens on your Mac, after your yes to that one signing.
  It costs the Developer ID case, which waits for you, and cloud signing needs an Admin key.
- Your certificates in the computer's keychain, as on a CI machine. Everything works with no wait.
  It costs the Developer ID key: an agent can copy it and sign anything as you, and only revoking
  the certificate undoes that.

Recommended: none of your account's keys. Most of an agent's work needs none, and the one key that
must never leak is the one that waits.

## Notes

What each key would let an agent do:

| Key | What an agent could do with it |
|---|---|
| None (simulator and local builds) | Nothing |
| Development certificate | Install its own builds on your registered devices |
| App Store distribution | Little alone: an upload also needs App Store Connect |
| Developer ID | Sign software that Macs then trust as yours |
| App Store Connect key | What its role allows: with Admin, revoke certificates and add users |

An agent's research, 24 November 2026: `research/2026-11-24-mac-and-ios-developers/README.md`, section "Signing, and which
keys may enter a computer". Developers report that cloud signing with a Developer ID certificate
does not work with an App Store Connect key, and that the cloud-signing permission cannot be given
to a key below Admin. Mac test 26 checks that `codesign` signs in a computer over SSH with no
dialog.

A signing on your Mac could also be a token inside the computer whose key operations go out to
your Mac, as a smart card does through CryptoTokenKit. Each signing would then be a fact in the
record. That is a design to try, not a decision.

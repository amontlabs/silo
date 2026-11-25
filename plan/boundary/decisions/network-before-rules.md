# Main's network before Silo has rules

## The choice

An agent's part, 24 November 2026. The first build of Silo has main, checkpoints and restore, but no
rules: `allow-hold-or-deny` and `open-limited-or-closed` come later. You make main and start Claude
Code in it. Claude Code has to reach `api.anthropic.com`, and `npm install` has to reach the
registry.

The options:

- Open with your yes, or closed, and say so (recommended). The first run asks once. Open reaches
  the internet, never your Mac or your local network. Silo says that nothing is checked at the edge
  yet. It costs the promise of `safe-fresh-install` until rules exist: a fresh main can be open.
- Closed until rules exist. It costs the first build: an agent program cannot reach its model
  provider, so nothing works.
- Open, saying nothing. It costs trust: you would read Silo's silence as a limit that is not there.

Recommended: open with your yes, or closed, and say so. An agent works, and you know what it can
reach.

## Notes

Agent, 24 November 2026. Drafts #352 (files 06 and 12) and #354 both found that the first build's
main is open from the first day, which `reaching-a-computer` ("nothing else is open") and
`channel-question` ("none exists by default"), both without a verdict, do not allow for. This entry
says it in one place.

Whatever carries the network in the first build, gvproxy as the first driver of `network-stack` or
Apple's NAT, has to keep the computer from your Mac. gvproxy maps an address in the computer to
your Mac's 127.0.0.1 by default, and its API answers from inside the computer without a token
(drafts #352, file 04, and #358). Both are turned off before main runs on it.

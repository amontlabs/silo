# How the first run reaches a new macOS computer before Silo's relay is in it

## The choice

An agent's part, 25 November 2026, from `research/2026-11-25-first-run-option-a/` (gap 1 there).
#378 proposes a network card for main that leads nowhere: its other end, on the Mac, answers ARP
for one router address and drops every other frame. The only way out is a proxy in main at
`127.0.0.1:3128`, carried over vsock to Silo on the Mac, and the way in is SSH carried over vsock
by the same relay. Apple's provisioning (`mac-silo-needs`) creates the user and turns on Remote
Login, which listens on the network card. On a card that leads nowhere, the first SSH has no road,
and the relay that would carry it must be installed over SSH.

The options:

- Apple's NAT for the first start only (recommended). Main starts on Apple's NAT, Silo connects over
  SSH, installs the relay and writes the proxy settings, then stops main and starts it again on the
  dead-end card. Every installer and every agent program runs after that, behind the proxy. It costs
  one more start (about two minutes, not measured), and a first two minutes in which main reaches
  your local network. Only Apple's setup and Silo's own steps run then. It goes against the letter
  of `network-stack`'s answer ("Never Apple's NAT") for those two minutes, so the record says so.
- gvproxy for the first start only. gvproxy with no outbound connection, forwarding only SSH in, as
  #370 describes its closed mode. It keeps `network-stack`'s answer, and costs #359's test 07
  (gvproxy under a macOS computer) as a blocker of the first run, which #378 freed option A from.
- A Mac end of the card that carries one TCP connection in, to main's port 22, and nothing out. It
  keeps both, and costs a small TCP implementation of Silo's own for one connection, built and
  fuzzed before the first run.

Recommended: Apple's NAT for the first start only. Nothing of yours has run in main yet, and it is
the only option that needs nothing new.

What is not known: whether Apple's provisioning can start a program given in its options, which
would let the relay start with no SSH at all. Neither #352 file 10 nor #378 found such an option.

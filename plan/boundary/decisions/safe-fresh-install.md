# What does a fresh install allow

## The choice

An agent's part, 22 November 2026. You install Silo, make main and start an agent in it, having set
nothing.

The options:

- Closed until you choose (recommended). A new computer reaches nothing. When you make main, Silo
  asks once how its network starts. It costs one question before the first agent can work.
- Open until you limit it. A new computer has the internet, like any virtual machine, and you narrow
  it later. It costs the first days, when an agent that read a bad page can send out what it finds,
  though it holds no secret yet and cannot reach your Mac.

Recommended: closed until you choose. One question at the start is cheap, and it is the only default
under which a computer never reaches more than you granted.

## Notes

"Safe" is among the words the record says not to use for the product. The sentence is copied as
the record has it.

## From option A's first-run transcript

Agent, 25 November 2026, from `research/2026-11-25-first-run-option-a/` (gap 3 there). With #378's network (a card that leads nowhere and a proxy on the Mac), a closed main is possible in the first build, and an open one is no longer "not checked": every connection is recorded by host, and model traffic is read. So #348's `network-before-rules` ("not checked, held or recorded") and #370's `silo status` (`network open, not checked`) are wrong under option A, and the first run's question can offer a real choice. The case against closed as the default in A: A has no rules and no asks, so a main limited to the providers stops the first `npm install` at night with nothing anyone can approve, and the agent spends the night on it. The transcript asks "open" or "providers only", with open as the default, and says what open records. Closed or limited as a default fits once `allow-hold-or-deny` and asks exist.

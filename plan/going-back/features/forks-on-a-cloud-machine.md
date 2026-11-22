# Forks on a cloud machine

## The choice

An agent's part, 22 November 2026. Main is a macOS computer on your Mac. You want five agents to try
five fixes at once, each in a fork of its own. Apple allows two macOS computers on a Mac, so only
one fork can run beside main.

The options:

- Build it, later (recommended). The forks run on a cloud machine, and you see them as any other
  fork. It costs the copy of main's disk over the network for the first fork, tens of GB for a macOS
  computer, the rent of the cloud machine, and your code and files leaving your Mac. Whether a cloud
  Mac can be had at a rent and a speed worth it is to find out.
- Leave it out. A fork runs where its parent runs. It costs parallel work: on a Mac, one macOS fork
  at a time, and as many Linux forks as its memory holds.

Recommended: build it, later. Computers will run in a cloud anyway, and forks are where one machine
runs out first.

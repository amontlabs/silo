# A computer's network: open, limited or closed

## The choice

An agent's part, 22 November 2026. An agent in main installs a package. Its install script tries to
send your files to paste.rs and to open 192.168.1.10, the storage box on your home network.

The options:

- Build it (recommended). Main is limited to github.com and crates.io, so both attempts are denied,
  and you can close main's network in one step. Even an open computer never reaches your Mac, your
  local network or your VPN. It costs you a list of hosts to keep for work that needs the network.
  It also costs the work that needs a server on your Mac or a service behind your VPN, which does
  not run in a computer.
- Leave it out. A computer has the network your Mac has. It costs every other limit Silo sets at the
  edge of a computer, since secrets, the hosts in the record and the age of packages all need it.

Recommended: build it. Full access inside a computer is only acceptable when what leaves it is
decided outside it, and this is where that starts.

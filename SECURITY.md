# Security policy

Silo runs agents in virtual machines on your Mac and decides what they can reach: your repositories, your credentials and the host itself. A flaw in any of those boundaries is a security issue, and reports are welcome.

## Report a vulnerability

Report privately through [GitHub private vulnerability reporting](https://github.com/amontlabs/silo/security/advisories/new). Do not open a public issue, pull request or discussion for a vulnerability.

Include what you can of:

- the Silo app or engine version (`v*` or `engine-v*` tag) and macOS version;
- the boundary you crossed and what an attacker gains;
- steps or a script that reproduce it;
- whether you plan to disclose it, and when.

You should get an acknowledgement within 7 days. Silo is maintained by one person, so a fix can take longer; you will hear how it is going, and you will be credited in the advisory unless you ask not to be.

## Supported versions

Fixes land on `main` and ship in the next release. Only the latest app release (`v*`) and the latest engine release (`engine-v*`) get security fixes.

## In scope

- Code running in a computer (the guest) reading or writing host files outside what Silo shares with it, or running code on the host.
- A guest reaching the host's loopback services, another computer, or the engine socket.
- A guest reading the real value of a stored credential, or using it on a domain it was not granted for.
- A guest using GitHub access beyond the repositories and permission selected for that computer.
- Another local user, or a process without access to `~/.silo/engine.sock`, controlling the engine.
- Tampering with Silo's update, release or guest image downloads.

## Out of scope

- Malware already running as your user on the Mac. It can use the engine socket and Keychain items as you can.
- Vulnerabilities in Apple's Virtualization framework, libkrun or MicroSandbox themselves. Report those upstream; tell us too if Silo makes them easier to reach.
- Data a guest sends to a domain you allowed, including a credential's own response echoed back. Silo has no egress filter yet, so a guest can reach any public host.
- Anything a [personal token](docs/SiloUI-GITHUB-PERSONAL-TOKENS.md) is already permitted to do: Silo passes the token's full permissions.
- Missing notarization of the macOS app; it is known and documented.

# Reading the traffic to package registries

## The choice

An agent's part, 22 November 2026. A rule on main denies npm packages younger than 7 days. An agent
runs `npm install` for a package published yesterday.

The options:

- Silo reads the registry traffic (recommended). For the registries the rule names, and only those,
  Silo opens the encrypted connection with a certificate main trusts. It sees which package and
  version is asked for, and denies the one from yesterday. Silo puts its certificate in main and in
  the stores that Node, pip and Java keep apart. It costs an install that fails, with nothing
  fetched, when a program does not trust that certificate. Silo also sees everything sent to those
  registries, such as the token of a private one. Which package managers keep a store of their own,
  and how each is given the certificate, is still to find out.
- Silo stands in for the registry. Main's package managers are pointed at an address Silo serves,
  and the real registry is closed to main. No certificate is needed. It costs a setup for each
  package manager in each computer, and an install fails when it names the real registry, by a URL
  in a lockfile or in a script.

Recommended: Silo reads the registry traffic. The same certificate already serves secrets and model
traffic, and nothing in main has to be set up to point at Silo.

## Notes

An agent's part, from drafting the feature `minimum-package-age` on 21 November 2026. A rule on
the age of packages is enforced at the boundary (`package-age-at-the-boundary`), and the age of a
package is in what a registry answers. `ending-the-encrypted-connection` says how the boundary
reads model traffic. No decision says it reads the traffic to registries.

## Which package managers trust the computer's own store (24 November 2026)

An agent's part, 24 November 2026. A test server answered for the registry hosts with a
certificate from a test authority found only in the system's store (Ubuntu, the
`ca-certificates` bundle). Each manager ran with no certificate setting of its own.

| Trusts the system store as it is | Keeps its own store |
| --- | --- |
| curl, Python `urllib`, pip 26.2 and Ubuntu's pip 24.0, cargo, go, gem, bun 1.4.4, yarn 1.22 | node 22 `fetch`, npm 10.9.9, pnpm 10.28: Node's bundled store |
| maven, through Java's own store, which Ubuntu fills from the system's | uv: its own bundled roots |

- Node, npm and pnpm take the system store with `NODE_OPTIONS=--use-system-ca`, or a file with
  `NODE_EXTRA_CA_CERTS`. Both worked.
- uv takes it with `UV_NATIVE_TLS=1`. Its own hint names the flag `--system-certs`.
- pip trusts the system store since 24.2, through `truststore`. Ubuntu's older pip is patched to do
  the same, and an unpatched pip before 24.2 would not.
- Java keeps its own store everywhere. Ubuntu copies the system's authorities into it, and a Java
  installed by hand, such as one from a tarball, does not get them.
- yarn 1.22 refused a certificate from an authority outside the system store, so it does check.

So a computer needs Silo's certificate in three places: the system store, Node's (by the
environment), and uv's (by the environment). A Java installed outside the distribution also needs
it in its own store.

## Swift packages and CocoaPods

An agent's note, 24 November 2026. Swift packages mostly have no registry: a version is a Git tag
fetched from GitHub, and the date on a tag's commit is written by whoever made it, so it is not a
fact. What the boundary could read instead is when GitHub first saw the tag, from GitHub's API,
which is GitHub's report. CocoaPods' trunk becomes read-only on 2 December 2026, so nothing new is
published there to judge. An app in a simulator keeps its own certificate store: if a host it calls
is one where the boundary ends the connection, Silo's certificate has to be added to each simulator.
Mac test 27 of `research/2026-11-24-mac-and-ios-developers/README.md` checks it.

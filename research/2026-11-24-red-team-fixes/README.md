# Fixes for the top attacks of the red team, built and measured

PR #358 ranked 21 ways a hostile agent gets past Silo's precautions. This folder holds working
prototypes of fixes for five of them, each run on Linux against real programs: Claude Code 2.1.331,
Codex 0.172.0, Chromium and gvproxy. Everything here is an agent's work of 24 November 2026. None of it is
a feature polarzero took. Each fix is a path to choose between, and the notes of the plan entries it
touches lay out the choice.

| Attack in #358 | What was built | Result | Folder |
|---|---|---|---|
| 7. Steal the subscription login in the computer and use it elsewhere | A login vault at the boundary: the computer holds placeholders, Silo holds the real tokens and refreshes them | Claude Code and Codex sign in, work and refresh unmodified. A copied login file is refused everywhere. A fork shares the login without signing either computer out | [`logins/`](logins/) |
| 2. A page on a forwarded port carries data out through the Mac's browser | Four layers of preview isolation, each measured on its own, in Chromium | 26 ways out with plain forwarding. 0 with a Silo page around the app plus a browser that Silo starts | [`preview/`](preview/) |
| 1 and 3. gvproxy's API opens ports on the Mac; `192.168.127.254` reaches the Mac's loopback | A 64-line patch to gvproxy, `-sealed`, compared with stock gvproxy and with a token on the API | Sealed: nothing on the Mac, its loopback or its LAN is reached, and the API is gone. The internet and Silo's own port forwarding still work | [`network/`](network/) |
| 6. Smuggle a request past the boundary, or send a body it cannot read | A strict HTTP/1.1 and JSON reader in Zig, for the boundary of #347 | Refuses all 46 hostile cases and reads all 49 real requests of Claude Code and Codex. 33 of the hostile cases are read two ways by Node, Go, h11 or a lenient host | [`strict/`](strict/) |

The plan entries these touch are open, and the notes of each now say what was measured:
`logins`, `ports-reached-from-your-mac`, `paths-out-are-shown`, `network-stack`,
`forwarded-byte-for-byte` and `ending-the-encrypted-connection`.

## 1. The login vault

### The case

You sign in to Claude Code with your Max plan inside a computer, as `logins` recommends. A
dependency the agent installed reads `~/.claude/.credentials.json` and posts it to its own server.
From there, the attacker uses your plan and keeps renewing it, because the file holds the refresh
token. Nothing of Silo's sees it: the copy left in an encrypted connection to a host the rules
allow, or in a name lookup.

### How the vault works

The boundary already ends the encrypted connection to the providers' hosts
(`ending-the-encrypted-connection`). The vault ([`logins/vault.py`](logins/vault.py)) adds four
things there:

1. **Sign-in.** The program signs in the vendor's own way. When the provider's token endpoint
   answers the authorization code exchange, the vault keeps the real tokens and gives the program
   placeholders instead. A placeholder has the real token's length and prefix. For a JWT it keeps
   the real header and claims, with a signature that is not one, so the program still reads its
   account and plan from it.
2. **Use.** A request to the provider's API with a placeholder in `Authorization: Bearer` leaves
   with the real token in its place.
3. **Refresh.** A refresh with a placeholder is answered by the vault. One real chain of refresh
   tokens serves every computer that shares the login, so two computers never sign each other out.
   The vault refreshes upstream only when the real access token is about to expire, one refresh at
   a time per login.
4. **Refusal.** A placeholder is bound to the computer it was given to. Used from another computer,
   or after it was retired, it is refused and recorded. Copied out of Silo, it is worth nothing,
   since no provider has ever seen it.

A login the vault did not see being made, such as a token you pasted into the computer, passes
unchanged and the record says so. The vault's record never holds a real token: the code checks it
on every write.

Codex has one more step. After sign-in it exchanges its ID token for an API key (`sk-proj-…`). The
vault catches that reply too and gives the program a placeholder key.

### The test

Fake Anthropic and OpenAI sign-in and API servers ([`logins/providers.py`](logins/providers.py))
do what real ones are known or likely to do: access tokens expire, refresh tokens rotate, and a
refresh token used twice revokes the whole login after a grace of 10 seconds. That last behaviour is
modelled: whether Anthropic and OpenAI revoke a login on reuse was not checked. The real
`claude auth login` and `codex login` run as an ordinary user in two network namespaces, `c1` and
`c2`, each its own computer. [`logins/scenario.sh`](logins/scenario.sh) runs seven steps, once with
the vault and once with the boundary passing logins through, as `logins` recommends today.

| Step | Claude Code, vault | Claude Code, passed through | Codex, vault | Codex, passed through |
|---|---|---|---|---|
| Sign in in c1, work | works | works | works | works |
| Hostile code copies c1's login file and uses it from the internet | access 401, refresh `invalid_grant` | access 200, refresh 200 | refused | access 200, refresh 200 |
| Every token expires, c1 works | works | signed out by the thief's refresh | works | signed out by the thief's refresh, signs in again |
| A fork c2 keeps the login (`fork-and-sign-in`), three rounds with expiry | both work every round | the second refresh revokes both | both work every round | both signed out after round 1 |
| c1's login file used in c2 | refused | (not run) | refused | (not run) |
| Real tokens in any file of the computers | 0 | the thief's file holds them | 0 | 3 in the thief's file, 6 at the end |

The numbers are in [`logins/results/`](logins/results/), with every request the fake providers
saw. With the vault, Claude Code made 26 requests that were swapped and 4 upstream refreshes; the
3 refusals are the theft and the misplaced file.

### What it costs

- **Anthropic's terms.** As read on 18 November 2026 (`logins`, `anthropic-reads-intermediate`),
  they forbid a developer to "collect, store, or intermediate" a subscription's login. The vault
  does all three. It works, and it is not allowed for a Claude subscription without an agreement
  with Anthropic. OpenAI's terms on this were not read.
- **A login lives in Silo.** The real tokens sit in Silo's keychain on the Mac. A copy of that is
  worth every computer's access.
- **A vendor change breaks it.** The vault reads each provider's token reply. A new field, a token
  bound to the device, or certificate pinning in the program would stop it until Silo follows.
- **Codex's WebSocket transport** fell back to HTTPS in the tests. The vault swaps only in HTTP
  requests; the swap in a WebSocket upgrade is the same header and was not tested.

### The paths

- **No vault (today's recommendation).** The login sits in the computer. Any code there can copy
  it and use it from anywhere, and a fork with the login signs both computers out once tokens
  rotate. A partial rule fits here: the boundary knows the token's value from the requests it reads,
  so it can refuse a request to any other read host that carries it (#358, A4). It does not see the
  value leave through an unread host or a name lookup.
- **The vault for API keys and for providers whose terms allow it.** Same code; an API key is the
  simple case of step 2.
- **The vault for a Claude subscription, with Anthropic's agreement.** Asking is polarzero's call.
  The prototype is ready to show what Silo would do.

## 2. Previews of a port, without a way out

### The case

An agent's dev server on port 3000 serves a page. You open it on your Mac. The page runs in your
browser, which the computer's network rules do not cover, with your Mac's localhost cookies and
services in reach.

### What was measured

A hostile page ([`preview/attack.html`](preview/attack.html)) tries 26 ways to send the computer's
secret to an attacker's host, two of them through a service on the Mac, and also starts a download
and opens popups. A Chromium driven by Playwright
opens it through Silo's forwarder ([`preview/servers.py`](preview/servers.py)) in seven set-ups.
The app's own work on its origin (fetch, WebSocket, storage, its own cookie, internal links) was
checked in each, and kept working in all seven.

| Set-up | Ways that reached the attacker | What is left |
|---|---|---|
| Plain: the port on `localhost:3000` | 26 | everything, the Mac's cookie and a Mac service included |
| A strict Content-Security-Policy added by the forwarder | 10 | navigation, links, popups, speculation rules, WebRTC |
| Its own origin per port, `3000.c1.silo.localhost` | 10 | the same 10; the Mac's cookies are out of reach |
| A Silo page that holds the app in a sandboxed frame | 3 | WebRTC only |
| The same, plus `webrtc 'block'` in the policy | 3 | Chromium ignores the directive |
| A browser Silo starts, whose every request goes through the computer's rules | 0 | 6 attempts denied and recorded; a download and popups still happen |
| The Silo page in the browser Silo starts | 0 | nothing: no download, no popup, no navigation away |

Three findings matter for the design:

- **A content policy does not stop a page from leaving.** Navigations and speculation rules ignore
  it in Chromium, so a policy alone leaves 10 ways.
- **WebRTC leaves from every page-level set-up.** A TURN allocation carried the secret in its
  username. Only the browser's own setting
  (`--force-webrtc-ip-handling-policy=disable_non_proxied_udp` with a proxy) closed it.
- **The frame holds the rest.** A sandboxed iframe with no top navigation, no popups and no
  downloads, inside a Silo page whose `frame-src` allows only the app's origin, stopped every
  navigation-based way.

### The paths

- **The Silo page in your own browser.** 3 ways left, all WebRTC. Your browser and its developer
  tools stay yours.
- **The Silo page in a browser Silo starts (recommended).** 0 ways, and what the page tries is
  denied by the computer's own rules and recorded. It costs your own browser for previews: its
  extensions, profile and history. On a Mac this is either a Chromium Silo ships or a WKWebView in
  Silo's app. A WKWebView's control over WebRTC was not checked on a Mac.
- **Plain localhost forwarding.** What `ports-reached-from-your-mac` says today. 26 ways out.

Each port gets its own origin in every path above but the last: two forks on port 3000 then get
`3000.c1.silo.localhost` and `3000.c2.silo.localhost`, which also settles the clash the entry
leaves open.

## 3. gvproxy, sealed

### The case

A computer runs with gvproxy as its network stack. From inside, one `curl` to
`192.168.127.1/services/forwarder/expose` makes gvproxy listen on every interface of the Mac
(#358, A1), and `192.168.127.254` reaches every port of the Mac's own loopback (#358, attack 3).

### What was built

[`network/gvproxy-sealed.patch`](network/gvproxy-sealed.patch) adds a `-sealed` flag to
gvisor-tap-vsock. With it, gvproxy serves no HTTP API on the gateway address, maps no address to
the host, and its forwarder refuses to dial loopback, private, link-local, CGNAT and multicast
addresses, and every address of the host itself. Silo still opens ports through the API on its own
unix socket, which nothing inside reaches. The project's own tests pass with the patch.

[`network/run.sh`](network/run.sh) puts a guest in a network namespace behind each build, with a
fake internet host at `203.0.113.10` and a service on the host's LAN address.

| From inside the computer | Stock gvproxy | API behind a token | Sealed |
|---|---|---|---|
| Open TCP ports on the gateway | 53, 80, 443 | 53, 80, 443 | 53 |
| The Mac's loopback through `192.168.127.254` | 6 ports, a secret page among them | 6 ports | none |
| The API opens a port on the Mac's every interface | yes, with no token | 401 | no API |
| A service on the Mac's LAN address | reached | reached | not reached |
| A host on the internet | reached | reached | reached |
| Silo opens a port from the Mac | works | works | works |

A token on the API fixes one row out of three. Sealing fixes all three.

### The paths

- **Seal gvproxy (recommended).** A small patch to carry, or to send upstream as an option.
- **Write the stack.** What `network-stack` describes in the end. Sealed gvproxy is what it does
  until then.
- **A token on the API.** Leaves the Mac's loopback and LAN open.

gvproxy's forwarder dials by address only, so it cannot apply rules by host name. Rules by name
stay the boundary's work, as `network-stack` has it.

## 4. A strict reader for the boundary

### The case

The boundary of #347 reads a request one way, and the provider's server another. #358 (A3) showed
a second request with a `web_fetch` tool passing the spike's record unseen.

### What was built

[`strict/strict.zig`](strict/strict.zig) reads a request whole, or refuses it. It refuses:

- anything but `METHOD SP /path SP HTTP/1.1`; a bare LF or CR; a control character; a folded line;
  a header name outside the token set;
- a header that may appear once appearing twice (Host, Content-Length, Transfer-Encoding,
  Content-Encoding, Content-Type, Authorization, x-api-key, Upgrade, Expect, Cookie);
- a Host other than the name in the handshake;
- a Content-Length other than digits; Content-Length together with Transfer-Encoding; a
  Transfer-Encoding other than one plain `chunked`; a chunk extension; trailers;
- an Upgrade other than WebSocket, and `HTTP2-Settings`;
- a body encoding other than gzip, deflate or zstd, and bytes after the end of the compressed body;
- in JSON: invalid UTF-8, a byte order mark, a lone surrogate, a key twice (even if only the case
  differs), an integer past 2^53, a number past the largest double, and nesting deeper than 128.

A body that is neither JSON nor a form is passed as unread, for the rules to decide.

### What was measured

[`strict/cases.py`](strict/cases.py) writes 50 requests: 4 ordinary and 46 hostile.
[`strict/diff.py`](strict/diff.py) sends each to Node's server, Go's `net/http`, Python's h11 and a
lenient parser, and each JSON body to the JSON parsers of Python, Node and Go.

- **The strict reader refuses all 46 hostile cases and reads the 4 ordinary ones.**
- **33 hostile cases are read two ways, or carry a hidden second request, in at least one of the
  other parsers.** Go takes `attacker.example` from an absolute-form target, where the others keep
  the Host header. Go and h11 accept bare LF line endings and folded headers, which Node refuses.
  Every parser takes the last of two duplicate keys; Go also matches keys case-insensitively, so it
  sees a `web_fetch` the others do not. Node reads an integer past 2^53 as a different number than
  Python and Go. The lines for every case are in [`strict/results.txt`](strict/results.txt).
- **All 49 real requests captured from Claude Code and Codex were read.** None was refused.

Brotli is refused, and neither program sent it.

### The paths

- **Refuse what is not strictly read (recommended).** A request to a host the boundary reads
  leaves only if the boundary read all of it. This is the property `boundary-reads-whole` that #358
  proposes, and it changes `forwarded-byte-for-byte`, which today lets an unreadable message pass.
- **Read like the host.** No single host parser exists to copy: the four above disagree on 33
  cases.
- **Normalize and re-send.** The boundary would write its own request from what it read. That breaks
  "byte for byte" and the provider's cache, and the hidden request is still read by nobody.

The reader is not yet wired into the spike of #347. That is the next step: apply it to the spike
and run #358's `boundary-tests/` and Claude Code through it.

## How to run

Every script expects Linux, root for the network namespaces, Python 3.13, Node with Playwright,
Go and Zig 0.15.2.

- `logins/`: `./mkcerts.sh`, `./net.sh up`, then `./scenario.sh cc /tmp/out vault` (or `cx`, or
  `pass`). Claude Code and Codex must be installed; they run as the user `cuser`, against the fake
  providers only.
- `preview/`: `./run.sh /tmp/out`.
- `network/`: build `cmd/gvproxy` and `cmd/gvforwarder` of gvisor-tap-vsock twice, with and without
  the patch, then `./run.sh <stock> <sealed> <gvforwarder> /tmp/out`.
- `strict/`: `./run.sh`.

## What went wrong while testing

- The first Claude Code run was started as root. It found an OAuth token of this test machine
  outside the test's home directory and sent it to the local fake provider. The fake's output was
  deleted, the value was copied nowhere, and every later run used the user `cuser` with an empty
  home. No request reached a real provider.
- With very short token lifetimes, Codex looped on "Account credentials changed during refresh".
  The tests use a one-hour lifetime and expire tokens on demand instead.

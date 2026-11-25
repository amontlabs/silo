# Port previews: the wrapper as one component, and the ways out #363 did not try

#363 measured 26 ways a page served on a computer's port leaves through your Mac's browser, and
0 ways once the page sits in a Silo page inside a browser that Silo starts. This follows on from
it. The wrapper is built here as one component, a forwarder plus one rule proxy per computer, in
[`prototype/silo_preview.py`](prototype/silo_preview.py). It was then tried against what #363 left
out: one computer's page reaching another computer, cookies, a worker an old computer leaves
behind, name lookups, dialogs, the clipboard, the browser's own traffic, and real dev servers
with hot reload.

Everything ran in Chromium (Chrome for Testing 158, headless) driven by Playwright on Linux. No Mac,
no WKWebView and no Safari were used.

## What was found

| # | Case | What was measured | What it changes |
|---|---|---|---|
| 1 | c1's page sets a cookie with `Domain=silo.localhost` | Under #363's names (`3000.c1.silo.localhost`), c2's app received c1's secret in that cookie, in your browser and in one browser shared by all computers. Removing `Domain=` from `Set-Cookie` in the forwarder did not stop it, because the page sets the cookie from script | Each computer needs a site of its own: `3000.<computer id>.localhost` |
| 2 | The same, with each computer as its own site | Nothing reached c2. Chromium refused `Domain=localhost` | |
| 3 | The app signs in with a cookie of its own (default `SameSite`) | It worked under both namings, as long as the Silo page is on the same site as the app: `silo.<computer id>.localhost/3000` | The Silo page is per computer too |
| 4 | c1 is dropped and a new computer takes the same name | A service worker c1's page installed answered the new computer's address, in the same browser profile. The new computer's own page was never served | Names are never reused: the computer's id, not its name. The browser profile of a dropped computer is deleted with it |
| 5 | The app's content policy allows `ws:` and `wss:`, as policies written for hot reload tend to | c1's page reached c2's app by WebSocket, in your browser and in one browser for all computers. One browser per computer, with its own rule proxy, denied it | A rule proxy per computer is the second wall. The policy must stay `connect-src 'self'` |
| 6 | WebRTC asks for a TURN server whose name holds the secret | In the browser #363 set up (proxy, UDP only through it), Chromium looked the name up through the Mac's resolver once per run, in 3 runs out of 3: the secret left as a DNS query. With `--host-resolver-rules="MAP * ~NOTFOUND, EXCLUDE 127.0.0.1"`, no lookup was made, and the name reached the rule proxy, which denied it without resolving it | #363's "0 ways" was 1. The browser Silo starts resolves no name itself |
| 7 | The app opens `prompt()` and `confirm()` that say "Silo: enter your Mac password" | With `allow-modals` in the frame's sandbox, as #363 had it, the browser drew both over Silo's page. Without it, neither appeared | Leave `allow-modals` out. An app that needs `confirm()` loses it in the preview |
| 8 | You click "copy" in the app | The page replaced what was copied with `curl … \| sh`, through the `copy` event, in every set-up. `navigator.clipboard` was refused | The clipboard is a way into the Mac that no set-up closes. It goes on `channel-list` |
| 9 | Notifications, fullscreen, a link to `vscode://` | Refused, refused, and the frame stayed where it was. Whether a headed browser on a Mac asks to open the editor was not seen | |
| 10 | What the browser sends by itself, 40 seconds idle | Nothing reached the rule proxy, with Silo's flags alone and with Playwright's | Chrome for Testing has no Google keys; a branded Chrome was not tried |
| 11 | Vite 8 and Next.js 16 dev servers, in the Silo page, in the browser Silo starts | Both loaded with no console error. An edit of the source reached the open page by hot reload in about half a second, without a reload. This worked whether the app saw `Host: 3000.c1a7.localhost` or `Host: localhost:3000` | Previews keep working with every layer on. The forwarder does not need to rewrite `Host` for these two |

## The set-up this recommends

For each computer, Silo starts a browser with its own profile and its own rule proxy:

```
chromium --user-data-dir=<the computer's preview profile>
         --proxy-server=http://127.0.0.1:<the computer's rule proxy>
         --proxy-bypass-list=<-loopback>
         --force-webrtc-ip-handling-policy=disable_non_proxied_udp
         --host-resolver-rules="MAP * ~NOTFOUND, EXCLUDE 127.0.0.1"
         --app=http://silo.<computer id>.localhost:<forwarder>/<port>
```

- Port 3000 of the computer with id `c1a7` is `http://3000.c1a7.localhost`. Its Silo page is
  `http://silo.c1a7.localhost/3000`. Two forks on port 3000 get two addresses.
- The rule proxy lets through that computer's own previews and the hosts the computer's rules
  allow. It decides on the name and never resolves a name it denies. What it denies is recorded,
  as if the computer had tried it.
- The forwarder replaces the app's content policy with its own (`connect-src 'self'`), adds a
  permissions policy that turns off devices, the clipboard API and fullscreen, and lets only the
  Silo page frame the app.
- The frame's sandbox is `allow-scripts allow-same-origin allow-forms`: no popups, no downloads,
  no top navigation, no dialogs.
- When a computer is dropped, its preview profile goes with it.

With this, every way tried here and in #363 is closed, except the clipboard (case 8).

## Choices for polarzero

These are notes on `ports-reached-from-your-mac`, beside #363's.

- **Your browser or a browser Silo starts.** #363 left this open. In your own browser, under the
  names above and the Silo page, WebRTC still leaves (#363) and so does the `ws:` case if the
  policy is ever loosened. Recommended: the browser Silo starts, with an "open in my browser"
  button that is off unless the computer is open.
- **The clipboard.** Accept it and say so on `channel-list`, or have the Silo app hold what a
  preview copies until you paste it in the app. Chromium gives Silo no way to do the second. A
  WKWebView might; it was not tried. Recommended: list it as a channel now and leave the rest to a
  later feature.

## What was not verified

- Anything on a Mac: WKWebView (its proxy API, `WKWebsiteDataStore.proxyConfigurations`, exists
  since macOS 14; whether WebRTC honours it was not checked), Safari, a headed Chrome, and the OS
  asking to open `vscode://`.
- A branded Chrome's own background traffic.
- Dev servers other than Vite and Next.js, and apps that sign in through another site (OAuth). Under
  a closed computer that site is denied, as it would be for the computer itself.
- The forwarder opens one connection per request; its speed was not measured.

## Running it

```
cd prototype
NODE_PATH=<a node_modules with playwright> ./run.sh /tmp/out <a Vite app> <a Next.js app>
```

`run.js` holds scenarios 1 to 10, `devservers.js` scenario 11, and `strace` is used to see the
DNS queries in scenario 6. [`prototype/results.json`](prototype/results.json) and
[`prototype/dns.txt`](prototype/dns.txt) are the output of the run reported here.

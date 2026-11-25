# Ports of a computer reached from your Mac

## The choice

An agent's part, 22 November 2026. An agent in a fork starts the development server of your site on
port 3000. You grant that port to your Mac and open localhost:3000 in your browser.

The options:

- Build it (recommended). You see what the agent built in your own browser. It costs a channel: a
  page served from the computer runs in the browser of your Mac, with what that browser holds for
  localhost. Two forks that both use port 3000 cannot both have 3000 on your Mac, and what Silo does
  then is still to design.
- Leave it out. You look at the site on the computer's own screen, inside Silo. It costs you your
  browser, its extensions and its developer tools.

Recommended: build it. Editors and container tools all forward ports for this reason, and a fork you
cannot open in your browser is of little use for web work.

## polarzero's words (21 November 2026)

polarzero believed this was planned: "giving ports from a computer accessible on the machine, which
I believe is already planned."

## An agent's notes

- The channel list names ports (`channel-list`), and `kinds-of-action` files "open a port" as
  granting. No feature built it, so this entry was added.
- Opening a port is granting. You do it, or a rule does. A precaution never does.
- Bad paths to design: two computers listen on the same port number and both want it on the Mac's
  localhost. The port is already taken on the Mac. The computer is paused or stopped while a program
  on the Mac is connected.
- A computer that reaches a port of your Mac is the other direction. It is not in this entry, and
  `nothing-shared-from-your-device` is in force.
- `sign-in-from-a-computer` needs this entry.

## The wrapper as one component (25 November 2026)

An agent's part, from `research/2026-11-25-port-previews`, which builds on #363's preview research.
The wrapper was built as one component (a forwarder and a rule proxy per computer) and tried
against what #363 did not try, in Chromium on Linux.

- **Each computer needs a site of its own.** Under names like `3000.c1.silo.localhost`, a page in c1
  set a cookie for `silo.localhost` from script, and c2's app received c1's secret in it. With
  `3000.<computer id>.localhost` and the Silo page at `silo.<computer id>.localhost/3000`, nothing
  crossed, and the app's own sign-in cookie still worked in the frame.
- **Names are never reused.** A service worker left by a dropped computer answered the address of a
  new computer with the same name. The computer's id goes in the name, and a dropped computer's
  preview profile is deleted with it.
- **The browser Silo starts must resolve no name itself.** #363's set-up let one DNS query out per
  run, for a TURN server whose name carried the secret. With
  `--host-resolver-rules="MAP * ~NOTFOUND, EXCLUDE 127.0.0.1"` none left, and the name was denied
  by the rule proxy without being looked up.
- **One rule proxy per computer.** With a content policy loosened for hot reload (`ws:`), c1's page
  reached c2's app by WebSocket in a browser shared by all computers. One browser per computer, with
  its own proxy, denied it.
- **No `allow-modals` in the frame.** With it, the app drew "Silo: enter your Mac password" over
  Silo's page.
- **The clipboard stays open.** A click on "copy" in the app put `curl … | sh` on the clipboard in
  every set-up. It is a channel for `channel-list`.
- **Dev servers keep working.** Vite 8 and Next.js 16 loaded in the Silo page, in the browser Silo
  starts with all of the above, and hot reload carried an edit in about half a second.

Recommended, if the entry is taken: a browser per computer, with the names, flags and sandbox
above. Not checked on a Mac: WKWebView, Safari, and a headed Chrome.

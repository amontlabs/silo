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

## Previews measured against a hostile page (24 November 2026)

An agent's part. The prototype and its numbers are in
`research/2026-11-24-red-team-fixes/preview/`. A page served on the computer's port 3000 tried 26
ways to send the computer's secret to an attacker, in Chromium, through seven set-ups of Silo's
forwarder. The app's own fetches, WebSocket, storage and cookie worked in all seven.

| Set-up | Ways that reached the attacker |
|---|---|
| The port at localhost, as this entry says today | 26, the Mac's localhost cookie and a service on the Mac among them |
| A strict content policy added by the forwarder | 10: navigations, speculation rules, popups, WebRTC |
| An origin of its own per port, such as `3000.c1.silo.localhost` | 10, with the Mac's cookies out of reach |
| A Silo page that holds the app in a sandboxed frame | 3, all WebRTC |
| That page in a browser Silo starts, whose requests go through the computer's rules | 0; what the page tried was denied and recorded |

The case for a change: "at localhost" puts a page the agent wrote next to everything your browser
holds for localhost, and lets it leave by 26 ways the computer's rules never see. A suggestion for
the entry: each port gets its own name under `.localhost`, which also gives two forks on port 3000
two addresses, and opens inside a page of Silo's. Whether that page runs in your browser (3 ways left)
or in a browser Silo starts (none left, but not your own browser) is the choice. A WKWebView's
control of WebRTC was not checked on a Mac.

# macOS computers on another Mac: remote display options

Research, 2026-10-10. Source and documentation review only; nothing was
installed or run. Statements marked **unverified** come from general knowledge
or a single secondary source and need a live probe (see the end).
Follows [macOS guest engine](macos-guest-engine-2026-10-09.md), which put the
VM in Silo's own process with a local `VZVirtualMachineView`.

## Requirement

A macOS computer whose VM runs on a second Apple Silicon Mac (managed over SSH,
like the Linux computers there) and is viewed and controlled from the local
Silo, which may run on macOS or Linux. Linux computers already reach the local
webview as Selkies 2.0 over an SSH-forwarded WebSocket. Guests have SIP
disabled, Silo can write the system TCC database (Accessibility, Screen
Recording) and has SSH into the guest.

## Summary

| Path | Where the screen comes from | Works pre-login / Setup Assistant / Recovery | Private API | Browser client |
| --- | --- | --- | --- | --- |
| A1. Apple Screen Sharing (guest) | `screensharingd` in the guest | Login window yes (**unverified**); Setup Assistant and Recovery no | No | noVNC |
| A2. Sunshine, RustDesk, other streamers (guest) | ScreenCaptureKit in the guest | No | No | No usable browser client |
| B. `_VZVNCServer` (host process owning the VM) | The VM framebuffer | Yes (Tart documents Recovery and installation) | Yes | noVNC |
| Selkies | Linux only | n/a | n/a | n/a |

## A. Guest-side streamers

### A1. Apple Screen Sharing / Remote Management

- **Enabling non-interactively.** `launchctl enable system/com.apple.screensharing`
  only marks the service enabled; it neither starts it nor grants the TCC rights
  it needs ([cirruslabs/macos-image-templates#376](https://github.com/cirruslabs/macos-image-templates/issues/376),
  closed 2026-09-21, which keeps the manual Sharing toggle in the Tahoe, Sequoia
  and Sonoma templates). Since macOS 12.1 the service is gated by TCC: the
  `com.apple.screensharing.agent` client needs `kTCCServiceScreenCapture` and
  `kTCCServicePostEvent`, stored in the system `TCC.db`
  ([macops.ca](https://macops.ca/managing-screen-sharing-in-monterey-12.1/)).
  The MDM `EnableRemoteDesktop` command sets both to allow; without it the
  viewer sees a blank or stalled screen. `kickstart` configures Remote
  Management access and privileges, but since Mojave it only permits observing
  unless the service was switched on through System Settings
  ([Scripting OS X](https://scriptingosx.com/2018/09/apple-remote-desktop-screen-sharing-and-mojave/),
  [Jamf thread](https://community.jamf.com/general-discussions-2/dealing-w-screen-sharing-on-macos-10-14-and-10-15-13781)).
- **Fit with Silo's guests.** With SIP disabled the system TCC database can be
  written, which is the path both sources point to. Plan: write the two rows
  for `com.apple.screensharing.agent`, run
  `launchctl enable` plus `launchctl load -w` of
  `/System/Library/LaunchDaemons/com.apple.screensharing.plist`. Whether this
  holds on macOS 15, 26 and 27 guests is **unverified** and needs a probe; the
  exact `kickstart` flags that still work on 26 are also unverified (the
  sources cover 10.14 to 12.1).
- **Authentication.** Two modes. (1) Account login (RFB security type 30,
  Apple's Diffie-Hellman variant): Apple's own clients and noVNC. (2) Legacy
  "VNC viewers may control screen with password": a separate VNC password,
  stored weakly; Apple warns third-party clients may not encrypt keystrokes
  ([Apple Remote Desktop guide](https://support.apple.com/guide/remote-desktop/set-up-a-computer-running-vnc-software-apdbed09830/mac)).
  Characters beyond 8 are truncated in the VNC-auth comparison
  (**unverified**, one secondary source). Setting the legacy password through
  `kickstart -setvncpw` has been reported unreliable
  ([search digest of Apple Community and gist threads](https://gist.github.com/nateware/3915757)).
  Type 30 with a Silo-generated guest account password is the better fit,
  because Silo already controls the account and the stream is tunneled.
- **Encodings, quality, resize.** Not verifiable from documents. noVNC will use
  whatever Apple's server accepts from its list (Tight, ZRLE, JPEG, H.264,
  Raw). Retina behavior (framebuffer in backing pixels versus points) and
  whether it sends `ExtendedDesktopSize` so noVNC's `resizeSession` works are
  **unverified**. Apple clients use proprietary extensions noVNC does not
  implement (display selection, high-performance mode).
- **Gaps.** Needs a logged-in or login-window session (**unverified**); not
  available in Setup Assistant or Recovery; depends on guest TCC state that
  macOS updates have broken before; adds guest software to keep working.

### A2. Sunshine, RustDesk, others

| Tool | License | macOS host status | Browser client |
| --- | --- | --- | --- |
| [Sunshine](https://github.com/LizardByte/Sunshine) v2026.914.233613 (2026-09-15) | GPL-3.0 | Supported per its docs on macOS 14.2+ with ScreenCaptureKit and VideoToolbox; requirement tables "a work in progress". A Sequoia installer failure and a Boost build break were reported in [discussion #777](https://github.com/orgs/LizardByte/discussions/777). Gamepads need a separately licensed Virtual HID Broker | None official; Moonlight clients are native |
| [RustDesk](https://github.com/rustdesk/rustdesk) | AGPL-3.0 | Client and host exist | Web client is a separate product; AGPL conflicts with bundling in Silo |
| Selkies ([repo](https://github.com/selkies-project/selkies)) | MPL-2.0 | Linux hosts only (X11, Wayland). No macOS host mentioned in the README; no plan found | Yes, but only Linux |

None gives a browser viewer without adding a gateway such as Apache Guacamole
(not evaluated). They also need guest Screen Recording and Accessibility, so
they add nothing over A1 apart from codecs. Not recommended.

## B. Host-side framebuffer: `_VZVNCServer`

### The private API

Header dump from macOS 26.4 ([thatmarcel/macOS-26.4-headers](https://github.com/thatmarcel/macOS-26.4-headers/blob/main/headers/Virtualization/_VZVNCServer.h)):
`_VZVNCServer` has `port` (read-only `unsigned short`), `securityConfiguration`,
`virtualMachine`, `graphicsDisplay`, `start`, `stop`, and initializers
`initWithPort:`, `initWithPort:queue:`, and Bonjour variants with
`securityConfiguration:`. There is no bind-address parameter. Companion classes
`_VZVNCAuthenticationSecurityConfiguration` and
`_VZVNCNoSecuritySecurityConfiguration` exist in the same dump. The class has
been present since macOS 11.3 ([cmsj/ApplePrivateHeaders](https://github.com/cmsj/ApplePrivateHeaders)).

### How Tart uses it (study only, FSL)

[openai/tart `FullFledgedVNC.swift`](https://github.com/openai/tart/blob/main/Sources/tart/VNC/FullFledgedVNC.swift),
the whole implementation, about 35 lines:

1. A password of four random words joined by `-`.
2. `Dynamic._VZVNCAuthenticationSecurityConfiguration(password:)`.
3. `Dynamic._VZVNCServer(port: 0, queue: DispatchQueue.global(), securityConfiguration:)`.
4. `vnc.virtualMachine = vm`, then `vnc.start()`.
5. Poll `vnc.port` every 50 ms until it is non-zero (zero right after start).
6. Print `vnc://:<password>@127.0.0.1:<port>`.
7. `vnc.stop()` on exit.

In `Commands/Run.swift` the flag `--vnc-experimental` is documented as "useful
since this type of VNC is available in recovery mode and in macOS
installation"; with no `--graphics`, Tart calls
`NSApplication.shared.setActivationPolicy(.prohibited)` and runs the main loop
without UI (about lines 658 to 664). License: FSL-1.1-ALv2, "Copyright
2022-2026 OpenAI" (the repository moved from cirruslabs to openai). Competing
use is excluded, so no code reuse; the shape above is public API usage from the
Objective-C runtime and is trivial to write independently.

### Other users

| Project | License | Use | Notes |
| --- | --- | --- | --- |
| [Lume](https://github.com/trycua/cua/tree/main/libs/lume) `src/VNC/VNCService.swift` (lines ~75 to 110) | MIT | Same calls, queue `DispatchQueue.main`, optional fixed port, polls port 20 times at 50 ms, throws if a requested port was not obtained. Password defaults to a four-word passphrase | `--vnc disabled` policy (`src/VM/VNCPolicy.swift`) starts no listener. VNC runs in every display mode, including `--display none` |
| [ClarifiedLabs/macvm](https://github.com/ClarifiedLabs/macvm) | see repo | Isolated Objective-C shim, resolved with `NSClassFromString`, fails with a clear error if absent | Documents: the server binds all interfaces, so every running VM is LAN-reachable with its random password; there is "no public API to inject input into a headless macOS guest"; the server requires the client to advertise the `DesktopSize` pseudo-encoding or it drops the connection ("Raw-only list" hits an internal FIXME); its client advertises Raw, DesktopSize, Cursor, LastRect only |
| [phantom-vm/phantom](https://github.com/phantom-vm/phantom), [rudavko/cortl](https://github.com/rudavko/cortl), gay-pizza/diavirt, roblabla/canned-mac | not checked | Runtime lookup of the same class | Phantom: serves the framebuffer with no guest software, works during install and Setup Assistant |
| [Fred78290/caker](https://github.com/Fred78290/caker) `VNCLib` | AGPL-3.0 | A replacement RFB 3.8 server that captures an `NSView` (Metal), written to avoid `_VZVNCServer` | Shows the alternative is a view-capture server in the VM-owning process; AGPL, so reference only |
| UTM (Apache-2.0), VirtualBuddy (BSD-2-Clause) | | A GitHub code search on 2026-10-10 found no `_VZVNC*` use in either | Both show the VM in a native view instead |

Cua: `cua-vmm` (a Rust crate in the `trycua/cua` repo) drives Lume through
`lume serve`; Cua Spaces v0.8.0-beta.2 exists. Neither was examined further:
they sit above Lume and add nothing at the display layer.

### What works and what breaks

- **Works.** Recovery, installation and Setup Assistant (Tart's flag
  documentation; phantom's design note), because the framebuffer comes from the
  host. macvm drives Setup Assistant entirely over it.
- **Reconnect crash.** After a client disconnects and the screen changes, the
  next client connect asserts in `-[_VZVNCServer _setupVirtualMachineAccessor]`
  (reported in [connorch/skills#66](https://github.com/connorch/skills/pull/66)
  and [r3dbars/transcripted#1813](https://github.com/r3dbars/transcripted/pull/1813)).
  The workaround there is a daemon that holds the one connection. Silo's bridge
  would be that single long-lived client, which also fits a browser that
  reconnects.
- **Bind address.** All interfaces, no option (same two reports, macvm docs).
  Anything on the remote Mac's LAN can reach the port. Mitigation: random
  per-session password (already the norm), a firewall rule, or a loopback-only
  forwarder in front, none of which stops a LAN peer from reaching the real
  port.
- **Keyboard layouts.** Tart users report `--vnc-experimental` does not map
  non-US keyboard layouts correctly ([tart#1359](https://github.com/openai/tart/issues/1359)).
  Relevant: the owner uses AZERTY. Silo's viewer must send keysyms deliberately.
- **Version drift.** Tart 1359 reports behavior changes after macOS 27; the
  class still appears in macOS 27 betas (blacktop/ipsw-diffs symbol listing).
  There is no stability promise, and the dyld-resolved class can disappear.
- **Resize and Retina.** `_VZVNCServer` has a `graphicsDisplay` property and
  macvm traces `DesktopSize` events, so it announces framebuffer changes.
  Resizing the guest from the viewer would be done by Silo through the public
  `VZGraphicsDisplay.reconfigure(sizeInPixels:)` (**unverified**; macOS 14+,
  per framework knowledge), not by the VNC client. Retina: the framebuffer is
  the guest's pixel size; scaling is the client's job.
- **Encodings.** Which encodings the server sends to clients that offer more
  than Raw is **unverified**. If it is Raw only, a full-screen change costs
  width x height x 4 bytes per update, which hurts over SSH. SSH compression
  and small dirty rects mitigate this. Needs a probe with noVNC logging.
- **Single client.** Treat as one connection at a time.

## C. Headless hosting of the VM

| Question | Finding |
| --- | --- |
| Run with no window? | Yes. Tart: `--no-graphics` or VNC without graphics calls `setActivationPolicy(.prohibited)` and runs `NSApplication.run()`. Lume: `--display none` / `--no-display`. The framework links AppKit; Apple DTS: [not daemon-safe](https://developer.apple.com/forums/thread/841688) |
| LaunchDaemon (no user session)? | Not viable. A root daemon launching a macOS VM fails intermittently with "Unable to access security information" and `ctkd: unable to generate key ... SepKey ACL` (same thread); a sandboxed launchd service fails on the USB sandbox extension ([thread 786363](https://developer.apple.com/forums/thread/786363)) |
| LaunchAgent / app in a user session? | Yes, but requires the user's session. macOS 15 and later need an unlocked `login.keychain`, otherwise "Interaction is not allowed with the Security Server". Tart's documented fixes: log in once through Screen Sharing and enable automatic login, or run `security unlock-keychain`, or an empty-password keychain ([tart.run/faq](https://tart.run/faq/)) |
| Over SSH only? | Tart FAQ treats SSH as workable once the keychain is unlocked. macvm (a Silo-like host app) says `run --headless` needs a logged-in GUI session because the app owns the VM, and its LaunchAgent "runs only in an Aqua login session" ([macvm docs/automation.md](https://github.com/ClarifiedLabs/macvm/blob/main/docs/automation.md)) |
| Logged out or locked screen? | Logged out: no session, so no VM owner. Locked: the session still exists and the keychain stays unlocked in practice (**unverified**). Restoring saved state needs the Mac unlocked ([forum thread 782007](https://developer.apple.com/forums/thread/782007), keychain-protected state) |
| FileVault | Apple engineer: no headless reboot works with FileVault; use auto-login, screen sharing or SSH ([thread 737381](https://developer.apple.com/forums/thread/737381)) |
| HTTP daemon model | `lume serve` is an HTTP API (default `127.0.0.1:7777`, `src/Server/Server.swift` line ~456), many VMs per process, plus `--mcp` over stdio. Loopback-only is good for SSH forwarding. The VMs live in the serve process and need the same user session |

Conclusion for Silo: the remote Mac needs a user logged in (auto-login, locked
screen acceptable, FileVault off or the user present after reboot) and the
login keychain unlocked. The VM host would be a Silo-signed process in that
user's session started over SSH or from a LaunchAgent, with no window, running
the same `objc2-virtualization` code and owning an `_VZVNCServer`.

## D. Client: noVNC

- [noVNC](https://github.com/novnc/noVNC) 1.7.0 (2026-04-28), last push
  2026-10-05, 14k stars, MPL-2.0 (file-level copyleft; fine to bundle
  unmodified). ES modules (`"type": "module"`, `core/rfb.js`), documented for
  embedding ([docs/EMBEDDING.md](https://github.com/novnc/noVNC/blob/master/docs/EMBEDDING.md),
  [docs/LIBRARY.md](https://github.com/novnc/noVNC/blob/master/docs/LIBRARY.md));
  the library API is a single `RFB` object.
- **Auth.** `core/rfb.js`: VNC auth (2), RA2ne (6, RealVNC RSA-AES), Tight (16),
  ARD (30) with `_negotiateARDAuth`, plus the version string `003.889`
  (Apple Remote Desktop) handled. The credentials event asks for username and
  password for ARD. A past ordering issue (VNC auth picked over ARD when both
  are offered, so no username field) was reported fixed on master
  ([noVNC mailing list](https://groups.google.com/g/novnc/c/M68QMnlO5Cs));
  confirm on 1.7.0.
- **Encodings.** Decoders registered: Raw, CopyRect, RRE, Hextile, Tight,
  TightPNG, ZRLE, JPEG, H.264. Pseudo-encodings include DesktopSize,
  ExtendedDesktopSize (`resizeSession`), cursor, QEMU extended key events and
  extended clipboard (`rfb.js`).
- **Transport.** noVNC speaks RFB over a WebSocket, so a WebSocket-to-TCP shim
  is required (websockify in the usual case). Silo needs a small one in Rust
  next to its existing SSH forwarding: the local side listens on loopback,
  upgrades the webview's WebSocket and forwards bytes to the SSH-forwarded
  remote port. The `_VZVNCServer` port is only reachable through the remote
  Mac's address or `127.0.0.1` on that Mac, so an SSH direct-tcpip channel
  works for both.
- **Embedding in the Tauri webview.** Selkies' client is embedded as served web
  content and works in Silo's webview; noVNC's `RFB` class is plain ES modules
  and canvas with no server-side rendering, so it can be vendored under the app
  assets or served from the same local origin. Needs the same CSP allowance for
  the loopback WebSocket that Selkies uses. Unverified in WKWebView and
  WebKitGTK; noVNC supports Safari and Firefox/Chromium. Clipboard goes through
  the browser Clipboard API (`core/clipboard.js`), subject to webview permission
  rules.
- **Latency.** noVNC is a software canvas decoder: fine for typical desktop
  use, not comparable to Selkies' hardware-encoded WebSocket stream.

## Options compared

| | Guest Screen Sharing + noVNC | `_VZVNCServer` + noVNC | Own capture server (caker-style) |
| --- | --- | --- | --- |
| Pre-login, Setup Assistant, Recovery | No | Yes | Yes (needs a view that exists) |
| Guest changes | TCC rows, service enable | None | None |
| Private API | No | Yes | No (public view APIs, heavy) |
| Maintenance | Apple TCC drift per macOS release | Class could vanish per release | We own an RFB server |
| Tunneling | TCP 5900 in guest, reachable only via guest NAT address from the remote Mac | TCP port on the remote Mac | Same as private API |
| License | Apple | Apple (runtime) | Our code |

## Recommendation

Use the host-side `_VZVNCServer` as the single display path for remote macOS
computers, viewed through noVNC in Silo's webview:

1. The remote Mac runs the same Silo VM host code (second process mode, no
   window) in the user's session, owning the `VZVirtualMachine` and one
   `_VZVNCServer` with `_VZVNCAuthenticationSecurityConfiguration` and a random
   per-session password. Resolve the classes at runtime; fail with a clear
   "unsupported macOS build" message; no hard link.
2. The local Silo forwards the VNC port over SSH, runs a loopback WebSocket
   bridge, and shows noVNC. Keyboard goes through keysyms (AZERTY); resizes go
   to the host as a public display reconfigure, not through RFB.
3. Keep one long-lived bridge connection per computer to avoid the reconnect
   assertion.
4. Treat guest Screen Sharing (A1) as a probe-gated alternative rather than a
   second path: pre-grant the two TCC rows for `com.apple.screensharing.agent`
   and test type 30 login from noVNC. Adopt it only if the probe shows clearly
   better encodings than `_VZVNCServer`.
5. Do not use Sunshine, RustDesk, or Tart code. Lume (MIT) is a legitimate
   reference for the call sequence and `lume serve` for the daemon shape.

Why: it covers the whole lifecycle (installer, Setup Assistant, Recovery,
login window) with no guest software, mirrors what Tart, Lume, macvm and
others already do, and shares the display code with the local window case.

## Risks

1. **Private API.** No stability or documentation, can change in any macOS
   release (macOS 27 reports exist). Contained by runtime lookup and a
   fallback message. A caker-style capture server or A1 is the exit.
2. **Network exposure and bandwidth.** `_VZVNCServer` binds every interface of
   the remote Mac with no option to restrict it, and encodings may be Raw only.
   A LAN peer who guesses the password reaches the guest screen. Needs a
   per-session password and a firewall rule on the remote Mac, and a measured
   bandwidth over SSH.
3. **Session requirements on the remote Mac.** No LaunchDaemon; the user must
   be logged in with an unlocked login keychain, which conflicts with
   FileVault-on unattended reboots. Screen sharing the Mac itself is the only
   way to recover from a reboot.
4. **Apple license.** The macOS SLA excludes use for "terminal sharing, relay
   service" and similar services, and allows two VMs per Mac for development,
   testing and personal use. Using one's own other Mac is likely fine; Silo
   should not offer shared or multi-user remote hosts. Reconfirm before
   marketing it ([SLA text](https://www.apple.com/legal/sla/docs/macOSTahoe.pdf)).
5. **Input quality.** AZERTY mapping, key repeat and modifier handling through
   RFB keysyms are weaker than a native view; clipboard needs a separate
   channel (guest SSH works).

## Needs a live probe

1. Which RFB security types and encodings `_VZVNCServer` offers; Raw-only or
   more; actual bandwidth and latency through an SSH forward at 1920x1080.
2. Does noVNC 1.7.0 connect (DesktopSize requirement, `AuthenticationSecurityConfiguration`
   password auth), and does it survive a reload without the reconnect assertion.
3. Retina and resize: framebuffer size for a 2x display; `reconfigure(sizeInPixels:)`
   triggering `DesktopSize` to noVNC.
4. Headless VM start from an SSH session on a Mac with auto-login and a locked
   screen; same with the login window only (no session); with an unlocked
   versus locked keychain; state save and restore while locked.
5. Guest Screen Sharing with SIP off: TCC row insertion plus `launchctl`
   enable on macOS 15 and 26 guests, ARD type 30 from noVNC, encodings and
   resize behavior, behavior at the login window.
6. Whether the VNC port can be restricted to loopback (for example with `pf`
   or an application firewall rule) without breaking the framework's own use.
7. noVNC under the Tauri webviews (WKWebView, WebKitGTK): WebSocket on the
   loopback bridge, clipboard permissions, pointer lock.

## Probe and decision (2026-10-10)

Probe 5 ran against a Silo macOS computer (macOS 26.6.2 guest, SIP off) from
the host, with a minimal RFB client written for the test:

- `sudo launchctl enable system/com.apple.screensharing` and a `bootstrap` of
  its launch daemon over SSH start Screen Sharing on port 5900 with no prompt.
  The guest's NAT address is reachable from its host only.
- The server speaks `RFB 003.889` and offers security types 30, 33, 35 and 36.
  Type 30 (Apple Diffie-Hellman) with the computer's own account succeeds.
  Type 2 (VNC password) is not offered.
- ServerInit reports 2560x1600 at 32 bpp. Raw full frames arrive in about
  0.55 s on the host. ZRLE (16) is honoured: a full idle Retina frame is
  53 KB. Tight (7) and Hextile (5) fall back to Raw.
- Without TCC rows the frames are black. Rows for
  `kTCCServiceScreenCapture`, `kTCCServiceAccessibility` and
  `kTCCServicePostEvent` for `com.apple.screensharing.agent` and
  `com.apple.screensharing.daemon` (the bundle identifier of
  `screensharingd.bundle`), written with the existing guest TCC helper,
  give the real desktop. Keyboard events are accepted, and macOS shows its
  own "Your screen is being controlled" notice.

Decision: **guest Screen Sharing (A1) with noVNC**, not `_VZVNCServer`.

- It is a public, supported macOS service, and its port is reachable only
  from the remote Mac, through the guest's NAT network. `_VZVNCServer` is a
  private class that listens on every interface of the remote Mac.
- Its encodings are good enough for noVNC (ZRLE), and authentication uses the
  account Silo already holds.
- What it does not cover, the installer, Setup Assistant and Recovery, Silo
  automates on the remote Mac without anyone watching. If a remote setup
  fails, its log is readable from the controlling device.
- `_VZVNCServer` stays the exit if Apple removes Screen Sharing access for
  virtual machines or its TCC rows stop working.

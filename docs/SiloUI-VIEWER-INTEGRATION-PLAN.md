# Desktop viewer integration plan: clipboard, files, audio and resizing

Status: proposed 2026-10-03, approved in scope by the owner. Nothing here is
implemented. Research inputs are cited by path; every behavior marked **probe**
must be confirmed on a live Dev build before the phase that depends on it lands.

## Goal

Make the built-in Linux desktop viewer feel like a local window without giving
the computer new ways to reach the device:

1. Text clipboard in both directions.
2. Image clipboard in both directions.
3. File upload (drag onto the viewer or the Files page) and download (Files page).
4. Audio playback with a mute control, enabled only where the device's web
   engine can decode it.
5. The computer's screen follows the viewer window size, without disturbing
   computer-use agents.

Out of scope: microphone input. Phase 3 locks it off on the guest so the
current default cannot be used by accident.

## Principles

- **The computer is untrusted.** The guest serves both the Selkies server and
  the web client JavaScript running in the viewer webview, so anything the page
  sends to Silo is guest-controlled. Only Silo's own window, menus and Rust code
  may start a read or write of the device clipboard or a file transfer. Nothing syncs because a viewer is open.
- **The guest child webview keeps no Tauri capabilities.**
  `capabilities/desktop-viewer.json` grants commands only to `desktop-shell-*`;
  this plan adds none to `guest-*`. `desktop_viewer_bridge.js` keeps locking the
  web clipboard APIs.
- **Reuse what is pinned.** Selkies 2.0.0 already implements chunked clipboard
  (text and images), Opus audio and RandR resize. OpenSSH
  `sftp` already works through Silo's managed SSH route for local and remote
  computers. New code is integration and policy.
- **Same behavior for computers on other devices.** Clipboard, audio and resize
  ride the viewer's existing tunneled Selkies WebSocket; files ride
  the existing `guest.ssh` stream. No new `silo-remote` methods and no protocol
  version bump.

## Current state (verified)

- Selkies is launched with `--enable-resize=false --enable-clipboard=false
  --enable-binary-clipboard=false --file-transfers=none`
  ([desktop-service.py](../app/SiloUI/src-tauri/guest/desktop-service.py), `launch_selkies_streamer`).
  Audio flags are unset, so Selkies' defaults apply: audio on, and the
  microphone setting unlocked, which lets the server accept microphone data.
- The viewer is a `desktop-shell-<uuid>` window with React chrome
  ([linux-desktop-viewer.tsx](../app/SiloUI/src/desktop/linux-desktop-viewer.tsx)) and an
  unprivileged `guest-desktop-shell-<uuid>` child webview loading
  `http://127.0.0.1:<port>/` through the cookie-authenticated proxy
  ([desktop_viewer.rs](../app/SiloUI/src-tauri/src/desktop_viewer.rs),
  [desktop_proxy.rs](../app/SiloUI/src-tauri/src/desktop_proxy.rs)). The proxy
  forwards HTTP and splices upgraded WebSockets transparently.
- No host-to-page channel exists: no `Webview::eval`, no `postMessage` bridge,
  no reserved proxy route.
- **Stale configuration.** The viewer URL's `resize=scale&clipboard_seamless=false`
  are KasmVNC parameters. Selkies 2.0.0 reads only `token`, `offscreen_worker`
  and `socket_worker` from the URL; all other client settings come from
  `localStorage` keys prefixed by the sanitized origin and path
  (`selkies-ws-core.js` lines 712, 1069-1132 at tag 2.0.0). The "Clipboard fix"
  section of [SiloUI-DESKTOP.md](SiloUI-DESKTOP.md) also describes KasmVNC.
  Clipboard is currently off only because the server flag is off.
- Xvfb starts at `-screen 0 1440x900x24`; upstream Selkies starts Xvfb at
  8192x4096 so RandR can grow the screen.
- Wry 0.55.1 (Tauri 2.11.5) enables autoplay on both WKWebView and WebKitGTK.
  It also grants every WKWebView media-capture request
  (`wry_web_view_ui_delegate.rs`); capture is denied today only because the app
  has no `NSMicrophoneUsageDescription` or `com.apple.security.device.audio-input`
  entitlement. Keep it that way: adding either would let the guest page record.
- No clipboard crate is a dependency. `tauri-plugin-dialog` is used from Rust only.
- The guest has no `xclip`, `xsel` or `xdotool`.

Upstream sources: Selkies 2.0.0 (`3ec56fb`) for the clipboard protocol
(`input_handler.py` 8093-8300 and 7265-7350, `websockets_mode.py` 776-890),
resize (`websockets_mode.py` 2999-3027 and 6107, `display_utils_xrandr.py`),
audio (`settings.py`, `audio_control.py`, `selkies-ws-core.js` 4466-4481,
6259-6483, 8283-8305), automatic resize (`selkies-ws-core.js` 3104-3150 and
4235-4260).

## Phase 0: foundation and probes

### 0.1 Host-to-page and page-to-host bridge

One mechanism serves clipboard, audio and resize.

- **Host to page:** `Webview::eval` on the guest child, called only from Rust
  in response to a shell action. Payloads are JSON-encoded, never string-built.
- **Page to host:** one reserved route in the proxy, `/__silo/v1/<op>`, served
  by Rust and never forwarded to the guest. It accepts a request only when Rust
  has a pending, single-use nonce for that viewer and operation (issued in the
  same `eval` that asked the page to respond), with a byte cap and a short
  expiry. Unsolicited requests are rejected and logged. This keeps the guest
  page unable to trigger anything on its own.
- **Page helper:** extend the existing initialization script (rename
  `desktop_viewer_guard.js` to `desktop_viewer_bridge.js` or add a second script)
  with a small `__silo` object that wraps the Selkies client's public hooks:
  `window.selkiesTransport` (`.send`, `message` events) and same-origin
  `postMessage` verbs (`setMute`, `setVolume`, `resetResolutionToWindow`). Do not add to `patch-selkies-web-client.py`; it
  edits minified code under a hash guard.
- **Settings seeding:** the script writes Selkies `localStorage` keys before
  client scripts run: `clipboard_seamless=false`. Compute the prefix from `location.origin + location.pathname`
  at runtime, because the proxy port changes per attach. Remove the stale URL
  parameters from `viewer_url`.

### 0.2 Shortcut interception

Cmd+C/Cmd+V (macOS) must reach Rust before the guest page. The first choice is
custom Edit-menu items for `desktop-shell-*` windows in
[app_menu.rs](../app/SiloUI/src-tauri/src/app_menu.rs). If WKWebView consumes the
key equivalent first (**probe**), install a native key monitor scoped to viewer
windows: `NSEvent` local monitor on macOS, a GTK `key-press-event` handler on
the shell window on Linux. In-page `keydown` listeners are not trusted triggers,
because guest JavaScript can forge the signal.

### 0.3 Probe list (Dev build, disposable `e2e-*` computer)

Record results in `docs/research/viewer-integration-probes-<date>.md`.

| # | Question | Decides |
|---|---|---|
| P1 | Does the shipped Selkies `.deb` bundle match tag 2.0.0 for `selkiesTransport`, the `postMessage` verbs and the `localStorage` keys? | Bridge design |
| P2 | Does `Webview::eval` work on an `add_child` webview on WKWebView and WebKitGTK? Does the seeding script run before Selkies reads settings? | Bridge design |
| P3 | Do Edit-menu accelerators fire while the guest webview has focus (macOS, Linux)? | 0.2 fallback |
| P4 | With `--enable-clipboard=true`, seamless off and the guard in place, does any WebKit Paste popup appear? | Phase 1 |
| P5 | Is `AudioDecoder` with Opus available in WKWebView on macOS 14, 15 and 26, and in WebKitGTK on Ubuntu 24.04 (deb and AppImage)? Which GStreamer package supplies Opus? | Phase 3 coverage |
| P6 | Guest PulseAudio sink names, whether Selkies uses `pulsectl` or `pactl`, `/proc/asound/cards` empty, audio after checkpoint restore | Phase 3 |
| P7 | Can Xvfb grow beyond its start size through RandR? Guest memory cost of a 4096x4096 framebuffer limit. Xfce panel and maximized windows after repeated resizes | Phase 5 |
| P8 | `sftp` throughput through `msb ssh serve --stdio` (local) and `silo --remote-guest` (remote), 1 GiB | Phase 4 limits |
| P9 | Does Tauri deliver drag-drop events for the guest child webview on both OSes? | Phase 4 |

## Phase 1: text clipboard

**Guest:** `--enable-clipboard=true`. Selkies holds CLIPBOARD and PRIMARY
ownership in its long-lived process, through its bundled python-xlib with
XFixes events; no `xclip` is needed.

**Device clipboard:** add `arboard` directly (it is what the Tauri clipboard
plugin wraps; a direct dependency gives PNG round-trips and Linux selection
ownership control without a JS surface). Wrap it in `src-tauri/src/clipboard.rs`,
Rust-only. Linux needs the `wayland-data-control` feature for Ubuntu 24.04's
default Wayland session, and must keep the selection served after writing.
Record the choice and its gaps in [SiloUI-DESKTOP.md](SiloUI-DESKTOP.md) when it lands.

**Paste into the computer** (Cmd+V on macOS; Ctrl+Shift+V and a menu item on
Linux, since Ctrl+V is a normal guest shortcut):

1. Native handler reads the device clipboard (text, at most 1 MiB).
2. `eval` sends it through `selkiesTransport.send` as `cw,<base64>`, or as
   `cws`/`cwd`/`cwe` chunks above 16 KB. The server awaits the guest write.
3. `eval` then sends Ctrl+V through the Selkies key path.

**Copy from the computer** (Cmd+C on macOS; Ctrl+Shift+C and a menu item on
Linux):

1. Native handler forwards Ctrl+C to the guest and issues a nonce.
2. The page helper waits up to 1 s for the next `clipboard,` frame, and replies
   to `/__silo/v1/clipboard` with the text. Without a new frame it sends the
   last cached guest clipboard, so copying in a guest app's menu and then using
   **Copy from computer** in the toolbar also works.
3. Rust writes the device clipboard and shows a brief "Copied from <computer>"
   status in the toolbar.

The connect-time clipboard push from Selkies is cached but never written to the
device.

## Phase 2: image clipboard

`--enable-binary-clipboard=true`. Same flows as Phase 1:

- Into the computer: Rust reads the device image, encodes PNG, sends
  `cb,image/png,<base64>` or `cbs`/`cbd`/`cbe` chunks. Cap at 16 MiB encoded
  (the server allows 64 MiB).
- Out of the computer: the page keeps the last `clipboard_binary` or chunked
  payload; the nonce reply posts raw bytes. Rust decodes PNG, JPEG, WebP or BMP
  and writes RGBA through `arboard`. Reject anything over the cap or that fails
  to decode.
- `text/html` flavours are not forwarded in this phase; plain text only.

## Phase 3: audio playback

Sound is offered only where it works. Nothing is keyed to an OS version: the
viewer tests the engine it is running in.

**Detection.** At each attach the page helper reports whether
`AudioDecoder.isConfigSupported({codec: 'opus', sampleRate: 48000,
numberOfChannels: 2})` succeeds. Rust records the result for that viewer.

- **Supported:** the toolbar shows a speaker button. Mute calls the existing
  `postMessage({type:'setMute'})`; the choice is saved per computer in the
  desktop configuration (default: unmuted).
- **Not supported:** the helper sends `STOP_AUDIO` so the guest stops encoding,
  and the toolbar shows no sound control. Selkies 2.0.0 decodes audio only
  through WebCodecs, which WebKit gained in Safari 26, so on macOS 14 and 15
  this is the expected outcome unless P5 shows otherwise. The help page states
  which systems play sound.
- In both cases, when the viewer window is hidden or minimized the helper sends
  `STOP_AUDIO`, and `START_AUDIO` when it returns (if supported and unmuted).

**Guest:** pass `--audio-enabled=true` explicitly, `--audio-bitrate=64000`
(remote computers share one TCP tunnel with video), and
`--microphone-enabled=false|locked`. Selkies' unlocked default only hides the
client toggle; the locked value makes the server reject microphone data. Add
`pulseaudio-utils` to
[desktop-packages.txt](../app/SiloUI/src-tauri/guest/desktop-packages.txt) only if P6
shows Selkies falls back to `pactl`; a package change requires a guest image
bump.

**Viewer hardening:** the initialization script makes
`navigator.mediaDevices.getUserMedia` reject. This also stops the Selkies
dashboard's device list from asking for the microphone. Hide the guest
sidebar's audio settings.

**Linux packaging:** add the GStreamer package that P5 identifies (likely
`gstreamer1.0-plugins-base` for Opus) to the deb dependencies in
`tauri.linux.conf.json`, and confirm the AppImage bundles `libgstopus`. If a
device still lacks it, detection hides sound as above.

**Checkpoints:** guest audio is PulseAudio null sinks only; there is no
virtual sound device, so full-memory checkpoints are unaffected (confirm in P6).

## Phase 4: file upload and download

**Transport:** the system OpenSSH `sftp` client in batch mode, using the
managed SSH config and ProxyCommand that
[editor.rs](../app/SiloUI/src-tauri/src/editor.rs) already builds (`msb ssh serve
--stdio --no-start` locally, `silo --remote-guest` for remote computers). The
sftp-user patch makes the session run as the working account, so files are
owned by `silo`. Rejected: `msb copy` (starts stopped computers, writes
root-owned files, no progress API), exec with stdin (hand-rolled protocol), and
JSON frames over `silo-remote` (4 MiB frames). Selkies' own file transfer stays
`none`.

**New module** `src-tauri/src/transfer.rs`:

- Shared function returning the SSH alias and config for a computer, factored
  out of `editor.rs`.
- Upload: validate the destination with `files.rs` `valid_path` (allowed roots
  `/workspace` and the working account's `~/Downloads`), sanitize the host
  basename (no `/`, NUL, `.` or `..`, at most 255 bytes), check free space,
  `put` to `.<name>.silo-part-<id>`, then `rename`. Conflicts: Replace, Keep
  both (default, `name (1)`), or Cancel, in a Silo dialog.
- Download: resolve the guest real path and `lstat` it; refuse anything that is
  not a regular file under an allowed root. Save via the backend
  `blocking_save_file` dialog (pattern in `log_export.rs`) to a temp file
  beside the destination, then rename.
- Progress as Tauri events, cancellation by killing the `sftp` process group
  (as `owned_tunnel.rs` does) and deleting the partial file.
- Limits: single files first, 4 GiB per file and 8 GiB per batch until P8
  measures throughput. Folders in a later step with entry and size caps.
- Clear errors for a stopped computer ("Start this computer to transfer files")
  and a missing `sftp` binary.

**UI:**

- Files page ([computer-file-tree.tsx](../app/SiloUI/src/features/application/components/computer-file-tree.tsx)):
  Upload into the shown folder, Download on file rows. The same tree is used by
  `status-folder-picker.tsx`; keep the actions scoped to the Files page.
- Viewer: dropping files onto the viewer uploads to `~/Downloads` and shows
  progress in the toolbar. The child webview is separate from the shell window,
  so Rust handles its native drag-drop events (`Webview::on_webview_event`) and
  the shell window's, and tells only the shell a one-time upload token and file
  names. Uploads accept only such tokens, never paths. The guest page never
  receives host file contents or drop events.

## Phase 5: the screen follows the window

The computer's screen resizes to the viewer window, the way a local VM window
does, using Selkies' own automatic resize.

**Why this is safe for agents.** LCU, the computer-use layer, never relies on
a cached screen size. Every pointer action is expressed relative to a target
window. LCU reads the screen size (`_NET_DESKTOP_GEOMETRY`) once per action,
and immediately before sending input reads the window's geometry again and
refuses a point that no longer lies inside the window
(`lcu/linux_sky_service.mjs` 400-408, 594-607 and 678-700, test "a point valid
for the old geometry is rejected after the window shrank"). A resize during an agent action therefore produces a refused action
and a fresh look, never a click in the wrong place.

**Guest:**

- Start Xvfb with room to grow, `-screen 0 4096x4096x24`. Xvfb 21.1 fixes its
  RandR maximum at the start size (`hw/vfb/InitOutput.c`, `vfbRandRInit`), so
  today's 1440x900 start cannot grow. The framebuffer is one allocation of
  about 64 MiB; Selkies caps client requests at 4080 pixels. Then set
  1440x900 with `xrandr` at session start so a computer with no viewer keeps
  today's size. P7 measures resident memory.
- Launch Selkies with `--enable-resize=true` and `--use-css-scaling=true|locked`.
  CSS scaling sends the window size in logical pixels and stretches the canvas,
  so a Retina window does not quadruple the pixels the guest encodes in
  software, and the desktop stays at 96 DPI.
- Bump the desktop recipe version and update the 1440x900 receipt checks in
  `desktop-service.py`, `setup-desktop.sh`, `desktop-streamer-lock.json` and
  their Python tests to check the start size rather than a fixed size.

**Behavior:**

- Selkies debounces window resizes (500 ms) and sends the new size on connect.
  Keep the shell window's existing minimum size so the guest never drops below
  about 640x400 logical pixels.
- Closing the viewer leaves the size as it was, so an agent working without a
  viewer sees no change. The toolbar menu has "Reset to 1440x900", which sends
  `r,1440x900` through the helper.
- A second viewer of the same computer from another device would resize it to
  that window. Selkies honors resize only from its primary client; confirm in
  P7 that a later connection does not fight the first.

## Cross-cutting work

- **Guest image and existing computers.** Flag changes live in the desktop
  service the host pushes, so the existing streamer update path covers v4
  computers. A package change (Phase 3, if needed) requires image v5 and
  the documented update flow for older computers.
- **Remote compatibility.** Nothing adds bridge methods. Both devices already
  must run the same remote protocol version.
- **Tests.**
  - Rust: nonce issue, expiry and single use for `/__silo/v1`, byte caps, path
    and filename validation, conflict naming, sftp argv and fake `sftp`
    binaries (as runtime tests fake `msb`), Selkies argv per setting.
  - Frontend: toolbar states, sound hidden when unsupported, Files page actions
    scoped away from the folder picker, transfer progress and errors.
  - Guard script: `getUserMedia` always rejected, clipboard APIs still
    locked, in [linux-desktop-guest-guard.test.ts](../app/SiloUI/src/desktop/linux-desktop-guest-guard.test.ts).
  - New commands registered in `build.rs`, `main.rs` and the shell capability,
    with the permissions snapshot updated.
  - Live on the packaged Dev app, local and remote (Linux test machine):
    every probe, plus clipboard round-trips with Unicode and a 10 MB image,
    upload and download of a 1 GiB file, cancellation, symlink refusal,
    playback after checkpoint restore, sound hidden on macOS 14 or 15, and an
    LCU agent task while the viewer window is resized.
- **Docs.** Update [SiloUI-DESKTOP.md](SiloUI-DESKTOP.md) (replace the KasmVNC
  clipboard section), [viewer direction](SiloUI-DESKTOP-VIEWER-DIRECTION.md)
  rows for clipboard, files and audio, [Files](SiloUI-FILES.md), and the bundled
  help. One `"silo-ui": minor` changeset per phase.

## Order and dependencies

| Phase | Depends on | Guest change | Size |
|---|---|---|---|
| 0 Bridge, shortcuts, probes | — | none | M |
| 1 Text clipboard | 0 | flag | M |
| 2 Image clipboard | 1 | flag | S |
| 3 Audio playback | 0, P5, P6 | flags, maybe package | S |
| 4 Files | P8, P9 | none | M |
| 5 Screen follows window | 0, P7 | Xvfb, flags, recipe | M |

Phase 4 is independent of the bridge and can run in parallel with Phases 1-3.

## Open risks

- **macOS 14 and 15 audio.** If WKWebView lacks `AudioDecoder`, those systems
  get no sound control; everything else works.
- **Minified client hooks.** `selkiesTransport` and the `postMessage` verbs are
  not a documented API. P1 pins them; a Selkies upgrade must re-verify them.
- **Shortcut capture.** If neither menu accelerators nor native monitors see
  Cmd+V before WKWebView, paste falls back to the toolbar and menu only.
- **Guest-forged replies.** A guest can answer a copy request with arbitrary
  content. That is inherent to copying from an untrusted computer; the device
  clipboard is written only after the user's action, with size caps.
- **Wayland hosts.** `arboard` data-control support varies by compositor;
  verify on Ubuntu 24.04 GNOME.

# Desktop viewer implementation evidence — 2026-09-27

## Decision

Keep the Silo viewer as a small, detached observer and input surface for the VM's existing X11 desktop. Replace the KasmVNC stream/client only after a Selkies 2.0 integration passes the same-display, WebSocket, input, and packaging checks. Reuse the current Tauri shell, capability-free child WebView, authenticated loopback proxy, and SSH tunnel. Do not add agent arbitration, a second desktop-control protocol, or a new dashboard. The operator remains responsible for simultaneous human and agent input.

Treat LCU as a separate guest-agent qualification and packaging workstream. Silo currently installs Luda 0.3.4. LCU v0.4.0 is a new release with distinct prerequisites and registration behavior; it is neither a drop-in viewer nor a reason to replace Luda silently. A zero-setup image cannot include LCU's Linux payload alone and satisfy its documented prerequisites: the official ChatGPT desktop app must already exist in the guest. LCU says it does not install that app, and its release archives do not supply it. Any bundled app decision needs a separate source, license, and redistribution assessment.

## What the code already proves

The viewer lifecycle and transport are already usefully separated:

- [`desktop_viewer.rs`](../../app/SiloUI/src-tauri/engine/src/desktop_viewer.rs) creates one Silo shell per workspace and a nested incognito child WebView for the remote display. It establishes a local-only SSH forward, starts the authenticated proxy, and loads the child at the proxy origin. The child has no Tauri capabilities; navigation is restricted to the proxy origin. Window geometry is sent to the native layer, and detach drops the view transport without stopping the guest desktop.
- [`desktop_proxy.rs`](../../app/SiloUI/src-tauri/engine/src/desktop_proxy.rs) is the auth boundary: it uses a random same-origin cookie, retains guest Basic credentials in the host process, strips unsafe hop/auth headers, injects guest auth upstream, forwards HTTP and WebSocket streams, and tears down on detach. Current tests exercise auth, same-origin checks, HTTP bodies, WebSocket relay, and disconnect. Selkies must pass through this boundary; do not put guest credentials in the URL or frontend.
- [`linux-desktop-viewer.tsx`](../../app/SiloUI/src/desktop/linux-desktop-viewer.tsx) owns state, explicit start/stop/restart, reconnect, fullscreen, and a small action menu. `NativeLinuxDesktopViewer` polls guest state, measures the screen rectangle, and serializes attach/detach IPC. Preserve this shell and change only the display client integration and any evidence-backed status text.
- [`capabilities/desktop-viewer.json`](../../app/SiloUI/src-tauri/capabilities/desktop-viewer.json) grants commands to `desktop-shell-*`, not `guest-*`. The nested viewer remains a remote-content surface without access to Silo commands.
- [`remote_access.rs`](../../app/SiloUI/src-tauri/engine/src/remote_access.rs) routes `desktop.connect` to the owning machine and maps the remote VM identity to its local runtime. The same owner-mediated SSH path supports remote viewing; it does not expose a guest port publicly.

The current guest recipe is fixed Xfce/X.Org with KasmVNC, a 1440×900 framebuffer, `allow_resize: false`, and client-side `resize=scale`. This distinction is important: current window resizing changes only local presentation, not the guest desktop or its pointer coordinate space. Silo's docs report a successful disposable ARM64 macOS-to-ARM64 guest view, including ASCII and `café`, but CJK/IME is explicitly unverified and no live remote-owner or AMD64 acceptance is recorded. Frontend tests cover lifecycle and attachment order, not actual pixels, keyboard events, or IME.

Relevant code and tests:

- [`linux-desktop-viewer.test.tsx`](../../app/SiloUI/src/desktop/linux-desktop-viewer.test.tsx): lifecycle, explicit start, confirmed stop, reconnect without start, Luda repair state, and menu behavior.
- [`linux-desktop-native.test.tsx`](../../app/SiloUI/src/desktop/linux-desktop-native.test.tsx): no implicit start for stopped VM, resize/attach and detach sequencing, reconnect sequence, native macOS insets, and transport errors.
- [`desktop_proxy.rs`](../../app/SiloUI/src-tauri/engine/src/desktop_proxy.rs) unit tests: loopback HTTP/WebSocket boundary behavior.
- [`SiloUI-DESKTOP.md`](../SiloUI-DESKTOP.md): current fixed-resolution behavior, guest setup, existing live evidence, and known IME gap.
- [`SiloUI-LUDA.md`](../SiloUI-LUDA.md): current agent integration and its supported-profile setup.

The main implementation seam is therefore `desktop_viewer::connect` plus the child WebView URL/client contract. Guest service installation, endpoint credentials, and remote dispatch can remain behind the current `desktop.connect` response if Selkies can provide compatible loopback HTTP/WebSocket service semantics. Confirm that with a pinned image before changing the response schema. Do not assume Kasm's `resize=scale` option maps directly to Selkies options.

## Selkies fit and limits

Selkies 2.0's official [native install guide](https://docs.selkies.io/latest/native) describes an attach-to-existing-display mode: it installs its own runtime but does not install a desktop, display server, or audio server. The supported existing Linux display is X.Org; an existing Wayland display cannot be captured by this mode. The guide warns against resizing a display with a physical monitor and documents a resize command that changes guest resolution. The [web-client guide](https://docs.selkies.io/latest/components/web-client) offers an HTML client and an embeddable web core with controls that can be hidden. This is a plausible replacement for the Kasm client inside Silo's child WebView, not proof of compatibility with WKWebView/WebKitGTK, current cookie auth, or the Silo proxy.

Keep the current fixed guest framebuffer for the first integration. Treat viewer fit/zoom as local scaling and preserve pointer mapping. Do not translate every Tauri resize event into a guest resize: the display is shared with applications and agents, and current workflows depend on stable coordinates. If users later need a different guest size, make it an explicit guest setting or action and show its resolution separately from viewer zoom. Do not ship that setting until Selkies' exact behavior and coordinate mapping are live-tested.

Selkies also has a broad client surface (audio, clipboard, file transfer, microphone, shortcuts, etc.). The current Silo client disables implicit clipboard in WebKit because of browser permission behavior and exposes a manual clipboard UI. Keep one compact Silo toolbar and hide duplicate Selkies chrome where supported. Do not claim drag-and-drop, clipboard, audio, multi-monitor, screen-reader, or IME support based on the generic web-client feature list; qualify each in the actual native host WebView. A feature can exist in the browser but still fail through a native nested WebView, the private proxy, or the guest session.

## LCU vs current Luda: independent prerequisite

The repository's desktop installer calls `luda.setup --prefix /opt/luda --user silo --agent all --yes`, pinned by `luda-lock.json`; Luda registers the existing Silo profiles and attaches to the session through its own documented integration. The prior E2B LCU research is explicitly about an older LCU 0.2.1 experiment, so its packaging assumptions must not be carried forward.

The pinned [LCU v0.4.0 installation guide](https://github.com/0xpolarzero/lcu/blob/v0.4.0/docs/INSTALLATION.md), [adapter contract](https://github.com/0xpolarzero/lcu/blob/v0.4.0/docs/ADAPTERS.md), and [release page](https://github.com/0xpolarzero/lcu/releases/tag/v0.4.0) establish these points:

- Linux targets Ubuntu 24.04-compatible glibc on x86-64 or ARM64, with X11 and an account-owned D-Bus session. Native Wayland and musl are unsupported.
- The official ChatGPT desktop app must already be installed (default `/usr/lib/chatgpt`). The installer copies a private managed app generation into `/opt/lcu`; it does not download or install the source app. The published LCU archives do not contain OpenAI app binaries. This is a hard prerequisite for an LCU-enabled image and a blocker to claiming a self-contained zero-setup LCU image. The official source does not grant redistribution permission; do not infer one.
- `lcu setup --agent … --session direct` is the documented path when the agent runs inside the active guest desktop session. The guide says LCU launches the selected app's Node/CUA REPL directly; it does not require launching the ChatGPT Electron UI for this native path. This removes the earlier uncertainty about a visible app window, but it still requires the app installation as source/runtime content.
- LCU wraps the selected app's original CUA provider. Its adapter document explicitly states it does not implement screenshots, accessibility, input, browser control, or JavaScript execution itself. Its release testing is evidence for LCU's own guest matrix, not for Silo's image, Luda registrations, Silo viewer, or remote tunnel.

For a reproducible LCU 0.4.0 guest lock, the release assets are `lcu-0.4.0-linux-arm64.tar.gz` (SHA-256 `1c07c091f1fd81efec87478d77b3a203099c2480774894fe9480f98dcfa1d635`) and `lcu-0.4.0-linux-x64.tar.gz` (SHA-256 `e2293487cdb3b6a9cb12f08b7ceef24449b5b98ddca97e4b31155cb0a576e124`). The guide also requires Python 3.12+. It defines the official Linux app prerequisite as ChatGPT installed at `/usr/lib/chatgpt` by default, then copied into LCU's private `/opt/lcu` generation; LCU neither installs that app nor includes its binaries. The guide links ChatGPT's public download page, which currently shows Mac and Windows desktop downloads but no Linux package, so a supported way to provision that required guest app remains unresolved. After the app and harness are installed, `lcu setup --agent … --session direct` can run inside the active guest X11/D-Bus session, uses that session's native CUA runtime, and does not start the ChatGPT Electron UI. Sources: [LCU 0.4.0 installation guide](https://github.com/0xpolarzero/lcu/blob/v0.4.0/docs/INSTALLATION.md), [release and checksums](https://github.com/0xpolarzero/lcu/releases/tag/v0.4.0), and [ChatGPT desktop downloads](https://chatgpt.com/download/).

Recommended work split: first qualify the viewer transport independently against the existing guest image and pinned Selkies runtime. If LCU is desired, separately build an explicit disposable guest using a legitimately provisioned official app, then test LCU direct mode with one supported Silo harness/profile and the same live display. Compare that outcome with current Luda workflows. Do not make LCU a hidden dependency of opening the viewer, do not bundle or fetch the app as a viewer side effect, and do not call LCU and Luda interchangeable.

## Minimal experience

Keep the title, guest state/error, reconnect, fullscreen, and existing lifecycle actions. The display should open only when the guest desktop is already running; opening a viewer must not start the VM or desktop. Show a persistent, small connection state when the stream is connecting or disconnected. Reconnect reattaches to the same guest desktop and must not restart it. Closing the viewer detaches only the view.

The web surface forwards human keyboard and pointer input to the desktop. There is no separate Silo control toggle, input lease, agent pause, takeover ceremony, or coordination status. Humans may see an agent's actions and can also interact; concurrent input is the user's responsibility. Keep agent setup/repair in the current viewer shell only if it remains part of the chosen guest integration; it is independent of the viewer stream.

Do not map macOS Command to Linux Control globally. Terminal `Ctrl+C` is an interrupt and semantic shortcuts vary by application. Preserve normal WebView key behavior, then test the actual guest key events and document any explicit Linux keyboard profile only if the current mapping demonstrably fails. Test Option/Alt, dead keys, and IME composition instead of treating a generic “escape” modifier as established behavior.

## Test plan and release gates

Use a pinned Selkies build and a disposable guest. Run the existing Kasm path as a control with the same image/display/workload. Capture build identifiers and report each cell independently; current evidence does not justify claiming all host/guest combinations from one successful macOS test.

| Host/viewer | Guest | Required live evidence |
|---|---|---|
| macOS ARM64 / WKWebView | Linux ARM64 / Xfce X.Org | Attach to pre-existing display; readability at native and resized shell sizes; pointer coordinate mapping; input; close/reconnect preserves app and guest state. |
| Linux x86-64 / WebKitGTK | Linux x86-64 / Xfce X.Org | Same checks, including WebSocket and cookie/proxy behavior in WebKitGTK. |
| Linux ARM64 / WebKitGTK | Linux ARM64 / Xfce X.Org | Same checks on supported Linux ARM64 host package. |
| Remote macOS ARM64 controller | Linux guest owned by another Silo host, both guest architectures | Verify owner routing, identity, SSH forward, reconnect after network interruption, no direct/public guest port. |
| Remote Linux x86-64 controller | Linux guest on remote owner, at least one guest architecture | Verify Linux WebKitGTK and owner-mediated tunnel path; record unsupported host combinations rather than generalizing. |

For each usable cell, run these acceptance checks:

1. **Lifecycle and isolation:** stopped VM opens a status screen without starting it; running guest attaches; viewer close drops tunnel/proxy but leaves desktop apps running; reconnect restores the same session. Child content cannot invoke Tauri commands or navigate off the authenticated proxy origin. Guest credentials never appear in frontend URLs, logs, or ordinary status.
2. **Text and geometry:** render a known editor/terminal page with small and large text. Resize and fullscreen the host window, including a HiDPI display. Confirm guest resolution is unchanged, text stays readable at local fit, and clicks land on the intended guest coordinates at all tested scales. Record actual guest dimensions separately from CSS viewport dimensions.
3. **Keyboard and IME:** use a physical keyboard for Latin, French AZERTY, accented/dead-key text, German layout, and CJK composition/candidate/commit where an installed guest input method and host locale make the case valid. Verify `Ctrl+C` in a terminal interrupts a foreground process. Test Command, Control, Option/Alt, Return, Escape, arrows, and key repeat in a text editor and terminal. Capture independent guest-side event/text evidence; a CUA typing tool is not a keyboard/IME test.
4. **Clipboard and files:** explicitly qualify copy and paste in both directions, multiline and Unicode. Test file transfer and drag/drop only if the product chooses to expose them; current product documentation does not promise them. Confirm browser/native permission prompts and cancellation are understandable, with no silent clipboard access.
5. **Failure visibility:** stop Selkies while the desktop remains up, kill the network, restart the owner, and expire/revoke the proxy. The shell must distinguish disconnected stream from stopped desktop and offer reconnect without destructive lifecycle actions. Verify stale WebSocket cleanup and bounded retry behavior.
6. **Input responsibility:** demonstrate that viewer input reaches the shared display while an agent operates it. The UI states no exclusive ownership. No lease or server input gate is added for this scope.

Do not set an arbitrary CPU, memory, bitrate, or latency SLA from product intuition. Before comparison, record baseline and candidate host/guest CPU, process RSS/PSS, network bytes/bitrate, encode/decode mode, frame rate/drops if available, and input-to-visible-pixel delay. Measure idle, scrolling/video, and text-entry periods with identical framebuffer, content, duration, network path, and encoder settings; repeat runs and report sample counts, median, and spread. Compare each host/guest cell separately. These numbers establish a baseline; a performance requirement needs real workflow evidence.

## Work sequence

1. Pin Selkies package/runtime and license/source checks in a disposable Xfce X11 guest. Verify it attaches to the existing `:1` session and does not create a competing desktop or resize the guest implicitly.
2. Probe the client in the actual Tauri host WebViews with loopback proxy, random cookie, WebSocket streaming, and reconnect. Confirm no external navigation or untrusted new-window path.
3. Implement only the adapter seam if the probe succeeds: guest service endpoint/credentials, URL/client options, and any required static assets. Keep the Rust owner tunnel and proxy policy. Add a focused transport test and a frontend regression for “shell resize changes local view, not guest resolution.”
4. Run the live matrix above, starting with macOS ARM64 local and Linux x86-64 local, then Linux ARM64 and remote owner paths. Do not claim unsupported cells.
5. Make LCU adoption its own decision after guest provisioning and harness acceptance. The current Luda path remains the only integration shipped by Silo until that separate decision passes.

## Evidence limits

This is source/repository inspection, not a Selkies Silo prototype or a new live benchmark. Selkies documentation confirms supported modes and client options, not that Silo's authenticated proxy, WKWebView, or WebKitGTK behaves correctly with them. LCU's release documentation reports its own tests, not tests against Silo's guest recipe or harness wiring. Existing Silo live evidence is ARM64 macOS to ARM64 guest only; current CJK/IME, full clipboard, Linux host, x86 guest, and remote-owner behavior remain open acceptance work.

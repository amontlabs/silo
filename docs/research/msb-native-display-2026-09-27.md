# MicroSandbox native display: implications for Silo

Source review, 2026-09-27. No runtime was installed or executed, and no Silo VM
was changed. Recommendation: evaluate the native display as a local Mac option;
do not adopt the experimental runtime fork as Silo's production baseline.

## What the demonstration establishes

The [August 30 post](https://x.com/yaluotao/status/2094072621327659437) demonstrates
Omarchy in a MicroSandbox VM with a native Mac window and input. It uses libkrun
and Apple's Hypervisor.framework. The
[current project description](https://github.com/ya-luotao/msb-omarchy#architecture)
separates the Arch Linux ARM/Hyprland desktop from the runtime's virtual display,
input, clipboard and sound devices. Rendering remains CPU-based. A virtual GPU
device does not establish host GPU acceleration.

The [findings](https://github.com/ya-luotao/msb-omarchy/blob/main/docs/findings.md)
describe shared-memory frame delivery and input fixes. They also record two
checkpoint blockers: graphics/sound devices cannot quiesce, and systemd PID 1
workloads cannot use the runtime's workload freezer. Resident pause is not a
checkpoint that survives host restart. These are reported upstream-project
experiments, not measurements reproduced in Silo.

## Mapping to the checked-out Silo implementation

| Layer | Silo now | Demonstrated alternative |
| --- | --- | --- |
| Guest/session | Ubuntu 24.04, Xfce/X11 | Arch Linux ARM, Hyprland/Wayland and Omarchy shell |
| Desktop display | KasmVNC 1.5.0, SSH tunnel and authenticated proxy, child WebView | Virtual display frames delivered to a native Mac viewer |
| Product shell | React/TypeScript and Rust/Tauri | Silo can retain this shell; viewer integration needs separate work |
| Remote computer | Existing SSH-backed viewer connection | Still requires a network display transport |

Local evidence: [guest recipe](../../app/SiloUI/src-tauri/guest/setup-desktop.sh),
[viewer and SSH transport](../../app/SiloUI/src-tauri/engine/src/desktop_viewer.rs),
[proxy](../../app/SiloUI/src-tauri/engine/src/desktop_proxy.rs), and
[desktop contract](../SiloUI-DESKTOP.md).

The architectural opportunity is to remove video encoding, network transport and browser
decoding from the local display route. Lower latency and lower total resource
cost are hypotheses, not established advantages over Silo. Frame-rate reports
alone do not measure input-to-visible-response latency.

Ubuntu or another distribution can supply a guest for this architecture. This
is an inference from the virtual-device boundary, not a tested alternative
image. Kernel drivers, graphics packages, session startup and input integration
still need qualification. Xfce on a real virtual display would need a different
display-server arrangement from the current Kasm Xvnc session. Choosing GNOME,
Plasma or Hyprland is a separate desktop and agent-compatibility decision.

## Adoption constraints and reusable work

Practical distinctions, clarified from the
[implementation plan](https://github.com/ya-luotao/msb-omarchy/blob/main/docs/plan.md)
and [original display PR](https://github.com/superradcompany/microsandbox/pull/1482):

- Closing the viewer preserves running applications because the VM continues.
  Resident pause preserves their memory while retaining the VM process and RAM.
  Stopping the VM or restarting its host loses that running state; saved files
  persist. Application-specific session recovery is separate from VM capture.
- This requires source changes across MicroSandbox and its libkrun dependencies:
  frame delivery, a native viewer, input routing and device fixes, plus clipboard
  and Mac audio integration in later work. Flags select those features inside
  the fork; stock runtime flags do not supply the implementation. PR #1482 was
  closed by its author on September 23 because it was too large to review and
  had fallen behind upstream.
- Local shared-memory delivery does not extend directly across a network. The
  project also streams the same desktop through wayvnc. Silo can therefore
  design a common desktop and viewer interface with different local/remote
  delivery mechanisms; remote support need not retain KasmVNC specifically.
  Matching latency, clipboard, sound, scaling and shortcuts remains integration
  and verification work. A Linux remote host is not qualified by this Mac demo.

The [current boundaries](https://github.com/ya-luotao/msb-omarchy#current-boundaries)
limit supported hosts to Apple Silicon macOS. The repository explicitly calls
itself experimental, and reports that its large upstream proposals were
withdrawn unreviewed. Its MIT license covers the project; bundled components
retain their own licenses. Production adoption would require ownership of the
runtime changes, dependency updates, signing and platform qualification.

Security review must cover the new host-facing display/input/audio interfaces,
clipboard consent and shared-directory authority. Its unauthenticated loopback
VNC fallback must not replace Silo's authenticated remote-viewing contract.

Reuse the focused device/kernel findings upstream and the product ideas:
readable display profiles, configured applications, discoverable shared files,
and explicit clipboard/audio behavior. Desktop appearance can improve without
switching transport. A native window does not establish better task completion,
keyboard/IME behavior, accessibility or reliable human/agent handoff. Preserve
the existing [LCU and checkpoint requirements](checkpoints-desktop-direction-2026-09-24.md).

**Next action:** run one isolated Ubuntu/Xfce native-display experiment against
the current Ubuntu/Xfce/Kasm route, holding applications and resolution fixed.
Measure task/input latency, host-plus-guest CPU/memory, text clarity, clipboard,
reconnect and agent access. Require working checkpoint/fork and a maintained
upstream route before production adoption; the reviewed fork currently fails
the checkpoint gate.

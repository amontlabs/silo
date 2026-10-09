# Native local display: VMPal, Cua Spaces and the upstream route

Research, 2026-10-07 to 2026-10-09. Binary inspection, source review and
upstream status checks. Nothing was installed into Silo or measured against it.
This updates [MicroSandbox native display](msb-native-display-2026-09-27.md);
the resulting direction is in
[viewer direction](../SiloUI-DESKTOP-VIEWER-DIRECTION.md#revision-2026-10-09-native-local-display).

## Why Silo's desktop feels slower than a native VM app

Silo's guest draws Xfce into Xvfb (CPU only), Selkies encodes it, and the
WKWebView decodes and paints it. Every frame and every input event crosses that
path. `--use-css-scaling` also makes the guest render at logical pixels that the
webview stretches on a Retina display ([settings](../SiloUI-DESKTOP.md)), which
is the visible softness.

## VMPal 0.41 (inspected `/Applications/VMPal.app`)

- Swift app. The VM runs in `Helpers/VMPalMachine.app`, which links
  Virtualization.framework (`VZVirtualMachineView`, used for macOS guests) and
  Hypervisor.framework directly (`hv_vm_create`, `hv_gic_create`): its own VMM,
  not QEMU.
- Frames move through IOSurface into Metal textures drawn by an `MTKView`: no
  encoding and no per-frame CPU copy.
- Guest 3D: a separate `VMPal GPU` process with UTM's virglrenderer fork, ANGLE
  and DXMT. Windows support adds EDK2, libtpms and virtio-win drivers.
- Linux guests carry `spice-vdagent` for resolution and clipboard.

## Cua Spaces (trycua/cua `main` at `c90e946`)

Same product shape as Silo (Tauri app plus a SwiftUI shell, agent desktops,
local and remote hosts) but still a streaming design: `cua-spacesd` in the guest
captures the screen (ScreenCaptureKit on macOS guests), encodes low-latency
H.264 with hardware encoders where available, sends it over WebSocket and the
webview decodes with WebCodecs. VMs come from Lume (Virtualization.framework),
QEMU or containers. Its smoothness comes mostly from GPU-backed macOS guests.
The streaming crates are FSL-1.1-MIT: reuse ideas, not code.

## The libkrun route

MicroSandbox's libkrun fork already has virtio-gpu, a display backend API
(`krun_add_display`, `krun_set_display_backend`, EDID/DPI), virtio-input and
virtio-snd. MicroSandbox does not build them (`net`, `blk` only).

| Piece | Source | Status, 2026-10-09 |
| --- | --- | --- |
| Native window, keyboard, mouse | [microsandbox#1482](https://github.com/superradcompany/microsandbox/pull/1482), libkrun [#116](https://github.com/superradcompany/libkrun/pull/116) [#117](https://github.com/superradcompany/libkrun/pull/117) [#118](https://github.com/superradcompany/libkrun/pull/118) | #1482 and #117/#118 closed by their author as too large; #116 open |
| Hardware cursor | libkrun [#119](https://github.com/superradcompany/libkrun/pull/119) | Closed with #1482 |
| Runtime resize | [steamac `0012`](https://github.com/fxgl/steamac/blob/main/host/libkrun/patches/0012-virtio-gpu-resize-displays-at-runtime.patch) | Against upstream libkrun, not proposed to MicroSandbox |
| Host audio | [steamac `0014`](https://github.com/fxgl/steamac/blob/main/host/libkrun/patches/0014-virtio-snd-macos-CoreAudio-backend-and-runtime-audio-API.patch) | Same |
| GPU (Venus) and zero-copy scanout | [microsandbox#1194](https://github.com/superradcompany/microsandbox/pull/1194), steamac `0007`-`0009` | #1194 open since July |

[steamac](https://github.com/fxgl/steamac) is Apache-2.0. libkrun's macOS GPU
path is Venus only (Vulkan through MoltenVK); there is no virgl OpenGL. The
guest then needs a Venus-capable Mesa.

Upstream answer on [microsandbox#1805](https://github.com/superradcompany/microsandbox/issues/1805):
a maintainer says virtio-gpu support is in progress for 0.8.x, and the open
question is whether its libraries are bundled or installed separately. Display
output and input were not confirmed as part of that work.

## Constraints that still apply

- **Checkpoints.** MicroSandbox refuses a checkpoint when any attached virtio
  device does not support quiesce (`admit_resources` in
  `crates/runtime/lib/checkpoint/coordinator.rs`, main `21b0d57`). In
  MicroSandbox's libkrun (branch `krun`, `b35c7e1`, 2026-10-08) the GPU, sound
  and input devices do not implement `supports_quiesce`, so they inherit the
  default `false`. msb-omarchy reproduced the refusal on 0.7.2
  ([finding D8](https://github.com/ya-luotao/msb-omarchy/blob/main/docs/findings.md)).
  Pause and resume work with these devices attached. Silo needs quiesce support
  for them before shipping a native display.
- **Remote computers.** Shared-memory delivery does not cross a network; remote
  computers keep streaming.
- **macOS guests.** libkrun cannot run macOS. macOS guests need
  Virtualization.framework, a second engine whose `VZVirtualMachineView`
  supplies display and input.
- Lower latency is expected from removing encode/transport/decode but has not
  been measured in Silo.

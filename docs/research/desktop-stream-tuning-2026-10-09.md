# Desktop stream tuning: sharpness and input latency

Measurements and changes, 2026-10-09, for the streamed desktop that remote
computers and (until a [native local display](desktop-native-display-2026-10-09.md))
local computers use. Direction: [viewer direction, revision 2026-10-09](../SiloUI-DESKTOP-VIEWER-DIRECTION.md#revision-2026-10-09-native-local-display).

## How it was measured

Dev build (`Silo Dev`, debug bundle from the `selkies-sharp` worktree) on an
Apple Silicon Mac (Retina, device pixel ratio 2), one throwaway local computer
`e2e-stream` (guest image v4, 8 vCPUs, 32 GiB), viewer window 1200x820 points
(the desktop area is 1192x736 points). All data is from that live computer; no
fixture data.

- `SILO_DESKTOP_BENCH_DIR` turns on a development-only harness
  (`src-tauri/src/desktop_viewer_bench.rs`, absent from release builds). It runs
  `desktop_viewer_bench.js` in the open viewer page and writes a report.
- `scripts/desktop-stream-bench/responder.py` covers the guest screen with one
  window: a key swaps a red and a blue noise tile, another starts a full-screen
  scroll. The page sends the key through the Selkies socket and times it until a
  frame with the other colour reaches the video sink (read from a clone of the
  sink's track in a worker; WebKit's `drawImage` of a track-generator `<video>`
  returns a stale frame). 30 probes per run, 150-300 ms apart. The fixture logs
  when each key arrives, which splits the time into input and picture parts.
- Selkies' own statistics (encode, decode, fps, bitrate, RTT) are sampled for
  10 s of full-screen scroll and 10 s of an idle ordinary desktop; host CPU comes
  from `ps` (WebKit helpers, Silo, ssh, msb) and guest CPU from `/proc`.
- `scripts/desktop-stream-bench/transport.py` measures the viewer's SSH forward
  alone: 32-byte echoes through the same `ssh -L` and `msb ssh serve --stdio`
  path, idle and next to a bulk download.
- `run.py` drives a run and `summary.py` prints it. The evidence is in
  `app/SiloUI/src-tauri/target/verification/stream-bench/` (ignored, not
  committed).

## Findings

### 1. Periodic 0.5 s stalls in every relay (the visible latency)

Baseline: half the keypresses showed up in 14 ms, but 5 of 30 took 349-519 ms.
All of the extra time was on the input side (the key reached the guest late);
the picture side stayed at 6-10 ms. The forward alone, idle: 0.35 ms median
round trip and 1.7 Gbit/s, but one round trip in roughly 150 took 515-590 ms,
every 1.2-1.5 s.

Cause (bisected by a delegated investigation against MicroSandbox 0.7.6 at
`09df3d4b` with Silo's patches): the VM runner's metrics sampler
(`crates/runtime/lib/runner/metrics.rs`) takes a snapshot every second. The
snapshot asks libkrun for host-resident memory, which calls `mincore()` over the
whole guest-memory mapping (48 GiB of address space for a 32 GiB computer) and
spent about 0.5 s of kernel time per call on this Mac. It ran synchronously on
one of the runner's two tokio workers, which also carry every agent relay, so
SSH forwards, the desktop stream and its input all stopped together. Evidence:
the stall period equals sleep plus snapshot (1.5 s), the runner's two workers
accumulated 10 s of system time per 30 s, `sample` showed only `mincore` on
them, the stalls stayed with the OpenSSH client, bulk channel and stdio all
removed, and a two-worker tokio model with a 0.5 s blocking task reproduces
them while the same task on `spawn_blocking` does not. Upstream main
(`05a630e5`) still samples synchronously.

Fix: Silo carries `microsandbox-metrics-sampler-blocking-0.7.6.patch`, which
takes the snapshot on tokio's blocking pool (see
[runtime packaging](../SiloUI-RUNTIME-PACKAGING.md)). The scan itself still
costs about a third of a core while a computer runs; throttling host-residency
sampling is an upstream follow-up.

### 2. Soft text: CSS scaling

`--use-css-scaling=true|locked` made the guest render 1192x736 for a Retina
window and the webview stretch it 2x. With `false|locked` the client requests
2384x1472 and sends 192 DPI. Stock Selkies maps that to Xfce's `Xft/DPI` only:
sharp text, but the panel, desktop icons and window frames stay 1x and the
clock overlaps. Integer window scaling (`Gdk/WindowScalingFactor` 2, `Xft/DPI`
96, xfwm4 `Default-xhdpi`) gives a correctly sized, sharp desktop.
Silo patches Selkies' XFCE path to do that, the same way Selkies already
treats MATE (see [optional Linux desktop](../SiloUI-DESKTOP.md)). This is a
candidate upstream change.

### 3. Encoding a still screen

Selkies' default "Turbo" mode (`video_streaming_mode`) encodes every frame at
60 fps even when nothing changes. At 2384x1472 that kept Selkies at 60% of a
core on an idle desktop. `--video-streaming-mode=false` (damage-gated) brought
it to 4%, sent nothing while idle and did not change the input-to-picture time.

### 4. Things checked and left alone

- Decoding is already WebCodecs in a worker with `optimizeForLatency` and the
  hardware decoder (NV12 frames), presented through a `VideoTrackGenerator`
  `<video>`; no change needed.
- 120 fps capture did not lower the time from keypress to picture and raised
  encode time; 60 fps stays.
- Paint-over (higher-quality refresh of a still screen) did not cause the
  stalls; it stays on.
- Silo's loopback proxy now sets `TCP_NODELAY` on the WKWebView side so small
  input frames are never held back; the guest side is a Unix socket.
- Cua Spaces (FSL-1.1-MIT, ideas only): damage-driven capture, one latest frame
  per queue, decode-queue limits with keyframe resync, local cursor, and a
  glass-to-glass benchmark. Selkies already has the first four; the benchmark
  idea shaped the harness above.

## Results

Before = Silo `main` at `f328998e`. After = the merged changes. Same computer,
window and method.

| | Before | After |
| --- | --- | --- |
| Stream resolution | 1192x736, stretched 2x | 2384x1472, 1:1 device pixels |
| Keypress to picture, median | 14 ms | _pending_ |
| Keypress to picture, 90th percentile | 405 ms | _pending_ |
| Keypress to picture, max | 519 ms | _pending_ |
| SSH forward round trips over 50 ms (3000 idle) | 14 | _pending_ |
| Selkies CPU, idle desktop | 19% of a core (1x) | _pending_ |

## Remaining gaps

- Remote computers use the same stream over a real network; not measured here.
- Linux hosts (WebKitGTK) and macOS 14/15 WebKit were not measured.
- At 2x the guest's screen has 4x the pixels while a viewer is open, and
  computer-use screenshots grow with it; closing the viewer keeps that size and
  density, and the viewer's **Reset to 1440×900** returns to 1x.

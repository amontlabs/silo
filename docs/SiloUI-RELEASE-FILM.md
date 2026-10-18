# Silo release film

`SiloRelease` is a 59-second, 1920×1080, 30 fps release composition in
[`demo/`](../demo/). It leads with agent computer use, then shows local and
remote computers, familiar tools, SSH agent handoff, scoped access, local export,
and a browser preview. It replaces the website's original v11 tour and covers
every feature category with a chapter destination.
The film is silent, matching the existing [demo direction](SiloUI-DEMO-SCRIPT.md).
The original `SiloDemo` and `SiloSshDemo` compositions remain available.

## Editorial timing

| Time | Scene and copy | Visual evidence |
| --- | --- | --- |
| 0–3s | “A computer. For your agents.” | Silo mark and kinetic typography; “Linux computers on the devices you own.” |
| 3–11s | “A desktop they can use.” | Illustrated agent observes a Linux desktop, clicks a project-creation button, and checks its result inside the production Silo viewer. |
| 11–16s | “Your devices. One place.” | Production overview with local and remote fixture computers; an SSH connection diagram. |
| 16–21s | “Your tools. Your flow.” | Production Files view, illustrated Zed edit and save, and a development-server command. |
| 21–32s | “Your agents. Connected.” | Production SSH controls enable local/network access, copy the address, and show Save key file. An illustrated client connects using that address and key, then reads a remote project. |
| 32–37s | “The right repositories. You decide.” | Production GitHub view; OAuth repository selection and read-only defaults. |
| 37–42s | “The right credentials. In scope.” | Production Secrets view with a fixture token assigned to a computer and HTTPS domain. |
| 42–47s | “Keep a copy. Keep going.” | Production computer page for local stopped web, on its Checkpoints tab where Export saves the computer to an export file; shows capture and verification, then success. |
| 47–54s | “Build there. Open here.” | Production Network view followed by an illustrated browser using a forwarded local address. |
| 54–59s | “Give your agents a space of their own.” | Silo identity, website address, macOS/Linux availability, and MIT license. |

[`release-timeline.ts`](../demo/src/release-timeline.ts) owns the 1,770-frame
timeline. [`release-film.tsx`](../demo/src/release-film.tsx) owns composition and
motion; [`release-style.css`](../demo/src/release-style.css) owns its typography,
color, and layout. Entrances settle before the corresponding detail is shown.

## Product claims and fidelity

The Silo viewer, overview, Files, SSH, GitHub, Secrets, Backup, and Network surfaces import
production components. Their data and state changes are deterministic fixtures.
Guest desktop content, agent session, editor, terminal, browser, and computer
diagram are illustrations. The desktop scene explicitly says “Illustrated agent
session”; its successful task is an example, not a recorded agent benchmark.
Rendering does not call a computer, SSH, credential store, agent, or external service.

| Claim | Repository source and boundary |
| --- | --- |
| Linux computers on owned devices | [README](../README.md). Local macOS/Linux and remote SSH management are supported; the film makes no boot-speed claim. |
| Agent desktop tools | [Luda integration](SiloUI-LUDA.md) (since removed; LCU replaces it). Adding the optional desktop installed tools and skills for supported guest agents. Users install and sign in to the agents themselves; ordinary device SSH does not inherit guest MCP configuration. |
| Local and remote management | [Remote computers](SiloUI-CONNECTIONS.md). The owner runs Silo and accepts SSH. Connections do not synchronize credentials or migrate computers. |
| Editor and terminal workflow | [Editor handoff](SiloUI-EDITOR-HANDOFF.md), [terminal handoff](SiloUI-TERMINAL-HANDOFF.md), and [Files](SiloUI-FILES.md). The edit and terminal output are illustrative, not native application recordings. |
| Repository selection and read-only defaults | [GitHub implementation](SiloUI-GITHUB-IMPLEMENTATION.md). The copy explicitly names OAuth; [personal tokens](SiloUI-GITHUB-PERSONAL-TOKENS.md) use their full permissions. |
| Computer and HTTPS-domain credential scope | [Secrets](SiloUI-SECRETS.md). Allowed servers receive the real value; the film does not claim credentials can never be revealed. |
| SSH handoff | [Production SSH controls](../app/SiloUI/src/features/application/pages/ssh-access-panel.tsx) and [remote computers](SiloUI-CONNECTIONS.md). Key handoff and client are illustrations; the devices already have a network route. No device-agent MCP inheritance is implied. |
| Local export | [Production export controller](../app/SiloUI/src-tauri/engine/src/backup_controller.rs), [computer export and import](../app/SiloUI/src/features/application/components/computer-transfer.tsx) and [checkpoints](../app/SiloUI/src/features/application/components/checkpoint-panel.tsx). Uses the current native indeterminate Capture and verify phase and export file naming convention. The stopped local computer remains stopped; elapsed time is compressed. Restore is described, not simulated as completed. |
| Development-server preview | [README setup](../README.md#start-working) and [production Network page](../app/SiloUI/src/features/application/pages/network-page.tsx). Discovering a port and connecting it are separate steps; a local address requires forwarding. |
| Availability and license | [README installation](../README.md#install) and [LICENSE](../LICENSE). Platform requirements remain in the installation guide. |

## Render and verify

Use Node.js 24 with the dependencies installed as described in the
[demo README](../demo/README.md). Run from the repository root:

```sh
npm --prefix demo run typecheck
npm --prefix demo test
npm --prefix demo run stills:release
npm --prefix demo run render:release
```

The output is `demo/out/release/silo-release.mp4`. The same ignored directory
holds PNG stills and `composition.json`, which records dimensions, duration,
fixture provenance, and silent audio. The renderer exports H.264 with `yuv420p`,
CRF 17, and `muted: true`; it rejects compositions longer than 60 seconds.
The stills command samples 21 frames, including intermediate SSH states,
backup progress/results, port connection states, and the final frame. A full
render exports 14 representative stills and a poster. Both derive scene samples
from the timeline. Website regression tests compare every category/chapter link,
caption interval, and transcript start against that same timeline.

Before delivery, run the timeline tests and typecheck, inspect the stills for
legibility and clipping, then inspect the encoded video for dimensions, frame
rate, duration, absent audio, and transition continuity. Timeline tests cover
contiguous scenes and observation/action/result order. Rendering proves fixture
presentation, not live computer health, agent reliability, two-device operation, or
release readiness. Record completed verification separately from this plan.

The existing Remotion renderer is reused. Primary references checked on
2026-09-27: [`renderMedia()`](https://www.remotion.dev/docs/renderer/render-media)
documents programmatic video output and encoding options;
[`renderStill()`](https://www.remotion.dev/docs/renderer/render-still) documents
frame selection and image output. The implementation is
[`release-render.mjs`](../demo/scripts/release-render.mjs).

## Original 54-second cut verification, 2026-09-27

- Node.js 24.11.1: `npm --prefix demo run typecheck` passed; `npm --prefix demo test` passed all 11 tests.
- `npm --prefix demo run stills:release` and `npm --prefix demo run render:release` completed. Inspected the scene stills, intermediate desktop states, secret editor, and port connection before browser reveal. Chromium required execution outside the macOS filesystem sandbox.
- `ffprobe` confirmed H.264, 1920×1080, 30 fps, 1,620 frames, exactly 54 seconds, 4,254,180 bytes, and no audio stream. The encoded video reports full-range 4:2:0 (`yuvj420p`). Its report is saved as `demo/out/release/ffprobe-54s.json`.
- `ffmpeg -hide_banner -v warning -i demo/out/release/silo-release.mp4 -f null -` decoded the complete file without warnings or errors. Inspected an encoded contact sheet in the output directory.
- No packaged Silo bundle or live computer was used. These checks establish the film's rendering and fixture presentation only.

## Website replacement verification, 2026-09-27

- Node.js 24.11.1: demo typecheck passed and all 13 timeline tests passed. The new coverage test first failed because the old film omitted Backup and SSH.
- Rendered and inspected 21 stills, including SSH off, local/network SSH enabled, Save key file menu, client connection, remote file read, backup capture, and completed archive. Exported the 59-second film and matching poster.
- `ffprobe` confirmed 1,770 frames at 30 fps, H.264 1920×1080, exactly 59 seconds, 4,865,357 bytes, and no audio stream. Full `ffmpeg` decode completed without warnings or errors.
- Backup matches the current native single indeterminate Capture and verify phase. The archive and client are synthetic, and all operations are inert.
- Website typecheck, all 17 website tests (12 Node tests and 5 Vitest tests), and the production build passed. The build retains its existing warning about the interactive demo bundle exceeding 500 kB.
- Browser verification loaded the replacement media at its actual 59-second duration, played it, and loaded all 10 caption cues. All eight chapter buttons and all six visible feature-category links sought to their matching scenes without media errors. Escape paused playback, dismissed the dialog, and restored focus to its opener.
- The final chapter remained active at 58.8 seconds and cleared at the 59-second endpoint. At 320 px and 390 px viewport widths, the dialog and all chapter buttons stayed within the viewport without horizontal page overflow. These were browser interaction and layout-bound checks against the local website preview.
- The rendered, public, and built-site MP4 files share SHA-256 `0a8a90d549d5fbec71bffb1a4172416ed50602447d6d33d8cc6fc7605c35c898`. The website poster, duration label, category links, player chapters, captions, and transcript were updated together. No public deployment was performed.

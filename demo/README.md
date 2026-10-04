# Silo demo

## Release film

**SiloRelease** is the 59-second, silent 1920×1080 release film, with
animated brand typography, an illustrated agent desktop task, production Silo
interfaces, SSH agent handoff, local backup, and a browser-preview payoff. The source remains editable in the
existing Remotion studio. Use Node.js 24:

```sh
npm --prefix demo run render:release
npm --prefix demo run stills:release
```

Output: `demo/out/release/silo-release.mp4`. Generated review frames and
composition metadata are in the same ignored directory. See the
[release-film notes](../docs/SiloUI-RELEASE-FILM.md) for timing, sources, and
fixture boundaries. Existing demo compositions and exports remain available.

## Earlier product demo

A 47.5-second, 1920×1080, 30 fps Remotion product launch film. All app data is
simulated and all backend actions are inert.

The v11 cut keeps preparation, connections, the development server, and
browser preview through 26.5s. A 20-second SSH ending replaces the Zed/edit
sequence: enable SSH for the remote computer, add it in an agent client, then smoothly
pull back to both devices. Choose the repository and submit the prompt on the
laptop; the Office Mac's computer shows the agent's first directory and file reads.
The brand close runs 46.5–47.5s. The standalone SSH demo remains 27 seconds;
the main film shortens pauses and text entry while preserving its camera speed.

## Run

Use Node.js 24. From the repository root:

```sh
npm --prefix app/SiloUI ci
npm --prefix demo ci
npm --prefix demo start
```

Open http://localhost:3400 and select **SiloDemo**. Remotion supports playback,
frame-by-frame inspection, and arbitrary seeking.

```sh
npm --prefix demo run render
```

Output: `demo/out/silo-demo-v11.mp4`. The original 110-second export is preserved
at `demo/out/silo-demo.mp4` when already present. The first render downloads
Remotion's headless browser; restricted macOS devices need permission to launch it
outside the sandbox.

## Implementation and fidelity

| Surface | Implementation |
| --- | --- |
| Silo shell, GitHub and repository picker, secret list and editor, backup progress/results, connection form, overview, remote settings, network, status panel | Imported production components with fixture data |
| Terminal, editor, browser, device silhouettes, native notification chrome | Demo illustrations |

The secret and connection forms now use the production components. Frame-driven
adapters open the secret editor and fill the connection field through their DOM
handlers. The custom backup review is removed. Files uses the production ComputersPage, repository actions, and a fixture directory loader.
Repository popovers retain production markup and await layout before capture.

- `src/timeline.ts`: shot boundaries and fixture frame ranges.
- `src/fixtures.ts`: inert app data and actions.
- `src/product.tsx`, `src/preparation.tsx`: production components and recordings.
- `src/film.tsx`, `src/style.css`: composition and motion.
- `scripts/cues.mjs`: historical sound generator, unused.

## Verify

```sh
npm --prefix demo run typecheck
npm --prefix demo test
npm --prefix demo run stills
```

Timeline tests cover boundaries, action outcomes, and reverse seeking. Rendered
frames establish presentation against fixtures, not live computers, SSH, native
notification delivery, or release readiness. Production app behavior is unchanged.

## References

[Storyboard](../docs/SiloUI-DEMO-SCRIPT.md) ·
[Remote behavior](../docs/SiloUI-CONNECTIONS.md) ·
[Remotion compositions](https://www.remotion.dev/docs/composition) ·
[Rendering](https://www.remotion.dev/docs/cli/render)

Remotion dependencies are pinned together to 4.0.521. See the
[Remotion license](https://www.remotion.dev/license) for distribution terms.

## SSH feature demo

`SiloSshDemo` is a separate 27-second, silent 1920×1080 composition. It starts
in close-up: enable SSH for the Office Mac's computer, copy its address, save its key,
and add the connection in an illustrated agent app. Clicking Connect begins
one smooth camera pullback into a laptop beside the Office Mac. The app and
cursor keep moving in the same shot. Both computers stay visible for folder
selection, prompt entry, and the remote agent's first directory and file reads.
There are no cuts back to the close-up after the pullback.

```sh
npm --prefix demo run render:ssh
npm --prefix demo run stills -- --ssh
```

Output: `demo/out/silo-ssh-demo-v4.mp4`. Select **SiloSshDemo** in the existing
Remotion studio to scrub the sequence. The original **SiloDemo** remains available.

- `src/ssh-film.tsx`: frame-driven SSH setup, continuous camera pullback, and illustrated agent client.
- `src/ssh-timeline.ts`: shared action times, camera movement, and cursor motion with a pause before each click.
- `src/ssh-style.css`: client, folder picker, and key-dialog styling.
- `src/product.tsx`: production overview and SSH controls with inert fixture actions.

The LAN address assumes both devices already have a route to each other.
The private key filename, native dialogs, device hardware, owner-side computer view,
client, folder listing, connection,
and agent activity are illustrations. This is not an exact Codex or ZCode UI,
and it makes no real SSH connection, writes no key, and changes no computer state.
Production copy feedback uses an inert browser clipboard adapter. Verification
covers rendering and fixture presentation, not live SSH or agent execution.


## GitHub heading screenshot workbench

```sh
npm --prefix demo run showcase
```

Open [the showcase](http://localhost:3410/showcase.html). The composition is a
1600×1100 frame that scales to fit the preview pane. Three staggered windows
show the Linux desktop, Computers and GitHub access. A light/dark appearance control sits
outside the frame. The app remains interactive so folders and menus can be
staged before capture.

The Linux desktop leads with the production viewer and an illustrated Linux desktop with a quiet gradient wallpaper.
The browser and agent terminal are HTML illustrations of an example computer-use task,
not a recording or proof of a completed agent run. Computers establishes local
and remote management; GitHub shows per-computer repository permissions. All project
names and state are simulated. Two computers are local; `lab` runs on the fixture
Studio Mac. No native commands or real account connections are used.

- `src/showcase.tsx`: composition, preview controls and production UI mounting.
- `src/showcase-desktop.tsx`: production desktop viewer and illustrated agent scenario.
- `src/showcase-fixtures.ts`: deterministic project data and remote-aware directory loader.
- `src/showcase.css`: framing, background and screenshot-specific material appearance.
- `showcase.vite.config.mjs`: standalone browser workbench, reusing the app's Vite dependencies.

For a control-free, unscaled canvas, use `?capture=1` (and optionally
`&theme=light`); `.showcase-frame` is 1600×1100.

`?variant=screens` shows only the three product screens (Linux desktop,
Computers, GitHub) in the same arrangement, without heading, background or
labels, on a transparent frame cropped to the windows plus an 80px margin for
their shadows (1633×1113). It always uses the dark product UI. The shadows are
softened so the image reads on dark and light pages; it is used on amontlabs.com.

```sh
npm --prefix demo run capture:showcase
```

This starts (or reuses) the showcase server on port 3410, renders with headless
Chrome at device pixel ratio 2 through the Chrome DevTools Protocol, and writes
`docs/silo-showcase.webp` (3200×2200, the README image) and
`docs/silo-showcase-screens.webp` (the screens variant, WebP with alpha). It needs
Node.js 24, Google Chrome (`CHROME_PATH` overrides the macOS default path) and
python3 with Pillow for the WebP encoding (`PYTHON` selects the interpreter).
The screenshot composition explicitly shows the standard glass appearance even
if the capture device requests reduced transparency; production accessibility
behavior is untouched. The workbench imports actual Silo components rather than
reconstructing the UI. The existing Remotion film is a separate entry point.

Validation: `npm --prefix demo run typecheck` and `npm --prefix demo run build:showcase`.
Build output lives in ignored `demo/out/showcase/`.

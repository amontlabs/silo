# Current cut: v11, 47.5 seconds

- 0–26.5s: Preserve the v10 opening, preparation, remote start, server, port forwarding, and browser preview.
- 26.5–37.7s: Enable SSH on the Office Mac's VM, copy the network address, save its key, and configure the illustrated agent client on the laptop.
- 37.7–39.3s: Clicking Connect begins one continuous pullback into the laptop screen, revealing Office Mac beside it. Keep the standalone film's 1.6-second camera move.
- 39.3–43.5s: Stay with both devices. Connect over SSH, select the remote repository, type and submit the prompt on the laptop.
- 43.5–46.5s: The agent starts reading the remote repository; show activity inside Office Mac's VM.
- 46.5–47.5s: Brand close.

The standalone SSH composition retains its 27-second pacing. The main film
retimes pauses and text entry to 20 seconds; all UI, connection, and agent data
remain fixtures. No extra captions or audio. Previous exports are preserved.

---

# Previous cut: v10, 35 seconds

The network shot now runs 22–26.5s: start empty, discover port 5173 without a
click, show the production Connect to this computer tooltip, forward the port,
then show Open in Safari and open the local address. The Files shot runs
26.5–28s with about 0.44s over Open in Zed before clicking. The side-by-side
editing sequence remains 28–34s. Discovery and forwarding are separate operations.

---

# Current cut: v9, 35 seconds

- 0–12s: Existing opening, GitHub, secret, backup and Connections sequence.
- 12–15s: Empty connection field, single-step address paste with ⌘V, then connect.
- 15–22s: Two-computer remote start, notification, and development server.
- 22–25s: Forward port 5173, open the initial Hello, Silo page on the laptop.
- 25–28s: Production Files page shows hello-silo in the remote demo VM. Focus and click its real Open in Zed control.
- 28–34s: Zed on the left edits the VM file; Safari stays visible on the right. Save precedes the browser heading update. Hold the changed result briefly.
- 34–35s: Existing brand close.

Keep silent audio, neutral surroundings and direct cuts. All behavior is simulated;
no clipboard, VM, SSH, editor process, or browser navigation is invoked.

---

# Current cut: v8

Same 30-second scenario as v7, with plain neutral surroundings, no soundtrack,
no chapter wipes, no opening reveal, and no camera zooms. Both devices appear
together. Keep production UI interactions and state changes.

---

# Previous cut: v7, 30 seconds

Approved reference direction: Arc Max's pace and Raycast's visual finish.
Everything is a frontend fixture. Silo surfaces use production components;
external apps and macOS device/notification framing are illustrations.

| Time | Action |
| --- | --- |
| 0–1 | Silo mark and name reveal. No slogan. |
| 1–5 | On Office Mac, select acme/hello-silo and allow changes. |
| 5–8 | Edit SERVICE_TOKEN's allowed destination, then show its configured row. |
| 8–10 | Backup progresses to a completed archive. |
| 10–12 | Enable Office Mac Connections and show its address. |
| 12–15 | Connect from the laptop with the production connection form. |
| 15–20 | Start from the laptop menu-bar panel. Pull back to both devices; Office Mac starts and receives its notification. |
| 20–22 | Run the development server in the remote terminal. |
| 22–25 | Edit the remote project and save. |
| 25–29 | Forward the port; open the resulting page on the laptop. |
| 29–30 | Silo mark, name and repository address. |

Use stable UI during actions, short chapter wipes, a deliberate remote reveal,
and sparse original sound cues. No background music. Secrets and GitHub settings
belong to the owning device; the film does not imply credential synchronization.
The backup progress is time-compressed and all displayed computer states simulated.

---

## Previous revisions

# Silo demo: prepare once, work from anywhere

The current film is 40 seconds, entirely frontend, with one project and one
handoff from Office Mac to a laptop. It starts with an existing, stopped `demo`
computer containing `acme/hello-silo`. GitHub is already connected. Project
cloning, dependency installation, and SSH authorization are prerequisites,
not presented as work that Silo performed automatically.

## Script

| Time | On screen | Action and payoff |
| --- | --- | --- |
| 0–2 | Opening | Silo. “Build beyond one computer.” A brief line connects My laptop to Office Mac. |
| 2–7 | Office Mac · GitHub | Choose the project repository and enable changes only for it. |
| 7–12 | Office Mac · Secrets | Save a masked secret for `demo`, restricted to one HTTPS destination. |
| 12–16 | Office Mac · Backup | Back up the stopped computer and show the completed archive. |
| 16–18 | Office Mac · Settings | Enable Connections and copy its address. |
| 18–21 | Laptop · Silo | Connect Office Mac; reveal the stopped remote computer beside the local one. |
| 21–26 | Both devices | Start the remote computer from the laptop’s menu-bar panel. Show both views changing to Running and a status notification on Office Mac. |
| 26–28 | Laptop · Ghostty | Start the prepared development server inside the remote computer. |
| 28–30 | Laptop · Files | Open the remote project in Zed. |
| 30–34 | Laptop · Zed | Change the heading and save on Office Mac. |
| 34–38 | Laptop · Network → Safari | Connect the discovered port and reveal the edited page through its forwarded address. |
| 38–40 | Close | Silo. Your computers. One place. |

The film keeps each interaction in a fixed frame, with quick cursor travel, action/result cuts,
and a side-by-side two-device scene. It is silent. The port preview is the final
payoff; there is no subsequent editor or menu-bar tour. See the
[editing research](SiloUI-DEMO-EDITING-RESEARCH.md) for source-backed guidance and
the editorial decisions derived from it.

## Fidelity and boundaries

GitHub configuration, secret storage, and backups belong to Office Mac. The
laptop connection does not synchronize those settings or migrate the computer.
New secret variables are configured before boot; credentials are excluded from
backups. Allowed HTTPS destinations can receive the real secret through the
proxy. The film makes no claim that credentials are unreadable by that server.

Backup and startup progress are editorially compressed, not speed measurements.
The health observer polls every 30 seconds in the real app; notification timing
is compressed. Its title and body match the implementation. Successful backups
do not produce native success notifications. The demo does not claim they do.

Production components supply GitHub, configured secrets, export progress/results,
menu-bar computer rows, the app shell, inventory, remote settings, and networking.
Private interactive forms use controlled, seekable adapters. macOS chrome,
notifications, editor, browser, and terminal are illustrations. No backend,
Keychain, VM, SSH, or external service is called.

Implementation references: [remote ownership](SiloUI-CONNECTIONS.md),
[GitHub page](../app/SiloUI/src/features/application/pages/github-page.tsx),
[secrets](SiloUI-SECRETS.md),
[computer export and import](../app/SiloUI/src/features/application/components/computer-transfer.tsx),
[notification events](../app/SiloUI/src-tauri/engine/src/notifications.rs), and
[menu-bar controls](../app/SiloUI/src/features/status-bar/status-bar.tsx).

## UI fidelity correction in v5

The opening no longer switches crops while selecting a repository. Its sidebar
remains visible. The illustrative dropdown overlay was removed; the movie opens
the production GitHub combobox through its normal focus handler and captures its
actual Radix popup. Fixture updates still determine repository selection and
permissions. Other scripted form adapters are listed explicitly in the
[demo README](../demo/README.md).

## Opening in v6

The two-second title sequence is a demo-only addition. Typography settles in
one short entrance, then the laptop-to-Office-Mac link draws once. The first
production screen appears with its picker closed and cursor hidden briefly,
then begins the existing repository interaction. The sidebar stays fixed.

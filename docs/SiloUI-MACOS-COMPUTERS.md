# macOS computers

Silo runs macOS computers on Apple Silicon Macs through Apple's
Virtualization.framework, next to the Linux computers it runs through
MicroSandbox. libkrun cannot boot macOS, so this is a second engine. The
engine choice, framework limits and Apple's license terms are in
[macOS guest engine research](research/macos-guest-engine-2026-10-09.md).

This is a first vertical slice: create, start, view, stop and delete. Most
Linux computer features do not apply yet; see [not covered yet](#not-covered-yet).

## Behaviour

- **Availability.** The overview page shows a macOS computers section on Apple
  Silicon Macs only. On Linux devices and Intel Macs the commands report the
  feature as unsupported and the section is hidden.
- **Create.** New macOS computer asks for a name, CPUs, memory and disk size.
  Silo asks the framework for the newest macOS this Mac can run, downloads that
  restore image from Apple (about 20 GB), and installs it. Progress shows as
  Preparing, Downloading and Installing. The creation form states Apple's
  two-copy license limit. After installation the user finishes macOS Setup
  Assistant in the computer's screen.
- **Start and stop.** Start boots the computer. Stop asks macOS to shut down,
  as the power button does; Force stop turns it off immediately. A shutdown
  from inside macOS shows the computer as stopped. Macs run at most two macOS
  guests at once; a third start fails with a message saying so.
- **Screen.** Show screen opens a Silo window containing a native
  `VZVirtualMachineView`: frames, keyboard, trackpad and audio come straight
  from the framework, with no encoder, network transport or webview. The guest
  display follows the window size. Closing the window keeps the computer
  running; stopping the computer closes it.
- **Delete.** Delete removes a stopped or failed computer and its disk. During
  creation the same action cancels it.
- **Quit.** Quit asks running macOS computers to shut down, then turns off any
  that have not stopped by the deadline, as it stops local Linux computers.
  A creation in progress is cancelled and shows as failed on the next launch.

## Storage

All paths are inside the channel's application data directory
(`~/Library/Application Support/org.silo.dev` for Silo Dev):

| Path | Contents |
| --- | --- |
| `macos-computers/<id>/computer.json` | Name, resources, MAC address, installed macOS version |
| `macos-computers/<id>/disk.img` | Sparse raw disk; uses only what macOS has written |
| `macos-computers/<id>/auxiliary-storage.img`, `hardware-model.bin`, `machine-identifier.bin` | The framework's per-computer platform identity |
| `macos-restore-images/<file>.ipsw` | The last downloaded restore image, kept so the next computer does not download it again; a `.partial` file resumes an interrupted download |

macOS computers are not entries in the MicroSandbox registry
(`computers.json`), so the Linux lifecycle, health, backup and Connections code
never sees them.

## Development and signing

Virtualization.framework requires the `com.apple.security.virtualization`
entitlement, which `Entitlements.plist` now carries. `npm run desktop`
(`tauri dev`) runs an unsigned binary without entitlements, so macOS computers
fail there; use `npm --prefix app/SiloUI run desktop:build:debug`, which signs
the Dev bundle ad hoc with the entitlements. Release signing verifies the
entitlement with the others.

The framework runs the computer inside the Silo process, on its main queue.
A crash of Silo therefore turns its macOS computers off abruptly.

## Not covered yet

| Silo feature | Status for macOS computers |
| --- | --- |
| Checkpoints | None. Framework save/restore state (macOS 14+) is tied to this Mac and needs a paused computer and a configuration that passes `validateSaveRestoreSupportWithError`; disk clones (APFS `clonefile`) are the likely checkpoint route. Not designed yet |
| Remote computers (Connections) | None. The view must live in the process that runs the computer, so a computer on another device would need a streamed or VNC path, and Apple's license excludes service-style use |
| Agent computer use | None. LCU, the agent harnesses and their installation are Linux-only in Silo today |
| Unattended setup | None. The user completes Setup Assistant by hand; no account, SSH or automatic login is configured |
| Terminal, editor, Files, network ports, GitHub, secrets, working account | None. These use the Linux guest bridge over SSH, which macOS computers do not have |
| Export, import and backup | None |
| Clipboard and shared folders | None. The framework offers a VirtioFS share and, for macOS 15 guests, a SPICE clipboard agent |
| Editing resources after creation | None. CPUs and memory are fixed at creation; disk size cannot change |
| Status panel, tray, notifications, start with Silo | None |
| Linux devices and Intel Macs | Not possible: the framework runs macOS guests only on Apple Silicon |

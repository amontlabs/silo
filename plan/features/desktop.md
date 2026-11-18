## Proposed

- Property: Windows and apps an agent opens stay inside the computer and never take focus or the screen on the user's device unless the user opens them.
- A recipe can turn on the desktop and computer use.
- Property: A computer built with its desktop on forks and checkpoints like any other.
- The image of a computer with a desktop includes a browser the agent can drive.

## Open

- To find out: Does the VM runtime give a Linux computer a real display, with a window manager or Wayland, given one report that microVMs do not and QEMU does?
- To find out: How much memory does a fork need with a browser running on its desktop, given that every fork pays it again?

## From the old code

The old code had it built and driven end to end, about 5,300 lines, and it needs reworking.

Lessons:
- A computer with its desktop off held its operation lane for 5 minutes after start waiting for a session, making its first fork take 288 s.
- Selkies 2.0.0 lets one page hold the display; a second page kills the first with a final close.
- A Linux recipe with desktop: false cannot have services, because services start with the desktop session.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src-tauri/engine/src/desktop.rs`
- `app/SiloUI/scripts/linux_desktop_services.py`
- `app/SiloUI/src-tauri/guest/desktop-service.py`
- `app/SiloUI/src-tauri/guest/setup-desktop.sh`
- `app/SiloUI/src-tauri/guest/patch-selkies-multiclient.py`
- `docs/direction/evidence/M.txt`
- `docs/SiloUI-RECIPES.md#the-desktop`
- `app/SiloUI/cli/src/cli.rs`

Calls: `desktop.status`, `desktop.action`, `silo desktop on`, `silo desktop off`.

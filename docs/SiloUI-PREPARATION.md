# Background preparation

At launch Silo prepares what each device needs before computers work well, without
blocking the app or holding the device-wide operation gate:

| Item | Owner | What it does |
| --- | --- | --- |
| Computer image | `engine/src/preparation.rs`, `guest_image.rs` | Downloads the image pinned in `guest-image/image-lock.json` (about 400 MiB, once), verifies its size and SHA-256, publishes it read-only and imports it into the runtime cache (about a minute). Nothing is downloaded when the cache already holds the image. See [guest images](SiloUI-GUEST-IMAGES.md#download-on-first-use). |

Silo no longer prepares anything else. The LCU archive and the ChatGPT for Linux app were removed from
background preparation on 2026-11-03 (see the [Cua Driver decision](SiloUI-COMPUTER-USE-PLAN.md#cua-driver-2026-11-03));
Cua Driver comes with the guest image, or is downloaded once by the guest helper inside the computer.

Evidence for the design is in
[background preparation flows](research/background-preparation-flows-2026-10-03.md).

## Behavior

- `preparation::start` runs once from the startup task, after the image cache repair and before any
  computer starts. Each item runs on its own thread; an item that is already prepared becomes `ready`
  with a metadata check only (the archive is neither downloaded nor hashed when the image is already cached).
- Every item is one `Job`: at most one run at a time. `ensure_image` joins a run in
  flight, returns at once when the item is ready, and otherwise runs it. A joiner of a failed run gets
  that failure; the next call tries again. `guest_image::prepare` still serializes imports with a
  process-wide lock, so a create that imports directly is safe beside the background import.
- State is `pending`, `running`, `ready` or `failed` (with a message and `retryable`). It is read with
  `read_preparation_status` and pushed as the `silo://preparation-status` event to the main window.
  `retry_preparation` starts again whatever failed.
- `msb image load` reports no progress, so the image import is indeterminate.
- The downloader lives in `engine/src/download.rs`: HTTPS only, at most five redirects, resume, retry
  with backoff. A size of 0 means "until the stream ends" and the checksum decides.

## Old downloads

Earlier versions published `<app data>/chatgpt/` and `<app data>/lcu/`. Computers created from the v4 image
still record those folders as read-only mounts, so the folders stay (empty) and Silo recreates them
before a start, restore or fork. At startup, when no computer is running, Silo empties both folders to
reclaim the disk (`engine/src/legacy_mounts.rs`). Export removes both mounts from the computer.

## Interface

The notification is `PreparationToast`, mounted in the application shell. `usePreparationStatus()` returns
the status, the items still in progress or failed, `ready`, and `retry()`, so an action that needs the
image can show a "waiting for" step. In the browser preview, `?preparation=downloading-image`,
`importing`, `failed` or `ready` selects a fixture (`app/SiloUI/src/fixtures/preparation.ts`).

## Tests

Rust: `cargo test --locked preparation` (job fast path, joining, failure and retry) and the loopback
download of unknown size in `download`, and the fast-path ordering in `guest_image`. Frontend:
`preparation.test.tsx` and `preparation-toast.test.tsx`. These use fixtures and prove no live import,
download or computer behavior.

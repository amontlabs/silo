# Background preparation

At launch Silo prepares what each device needs before computers work well, without
blocking the app or holding the device-wide operation gate:

| Item | Owner | What it does |
| --- | --- | --- |
| Computer image | `src-tauri/src/preparation.rs`, `guest_image.rs` | Downloads the image pinned in `guest-image/image-lock.json` (about 400 MiB, once), verifies its size and SHA-256, publishes it read-only and imports it into the runtime cache (about a minute). Nothing is downloaded when the cache already holds the image. See [guest images](SiloUI-GUEST-IMAGES.md#download-on-first-use). |
| LCU archive | `preparation.rs` | Downloads the archive pinned in `src-tauri/guest/lcu-lock.json` for the guest architecture, verifies its SHA-256 and publishes it read-only. |
| ChatGPT for Linux | `chatgpt_app.rs` | Unchanged. Its status is shown beside the other two. |

Evidence for the design is in
[background preparation flows](research/background-preparation-flows-2026-10-03.md).

## Behavior

- `preparation::start` runs once from the startup task, after the image cache repair and before any
  computer starts. Each item runs on its own thread; an item that is already prepared becomes `ready`
  with a metadata check only (the archive is neither downloaded nor hashed when the image is already cached).
- Every item is one `Job`: at most one run at a time. `ensure_image` and `ensure_lcu` join a run in
  flight, return at once when the item is ready, and otherwise run it. A joiner of a failed run gets
  that failure; the next call tries again. `guest_image::prepare` still serializes imports with a
  process-wide lock, so a create that imports directly is safe beside the background import.
- State is `pending`, `running`, `ready` or `failed` (with a message and `retryable`). It is read with
  `read_preparation_status` and pushed as the `silo://preparation-status` event to the main window.
  `retry_preparation` starts again whatever failed.
- `msb image load` reports no progress, so the image import is indeterminate. The LCU download is
  indeterminate too: the lock pins a checksum but not a size, so the downloader treats a size of 0 as
  "until the stream ends" and the checksum decides.

## LCU storage

Published as `<app data>/lcu/<version>/<archive>` (the app data directory is channel specific, see
[build channels](SiloUI-BUILD-CHANNELS.md)), file mode 0444 and directory 0555. A partial download
stays in `.download/` and resumes. Anything else in `lcu/` (older versions, unfinished staging) is
removed after a successful publish. `preparation::lcu_folder` returns the folder once the checksum
matches; the checksum is read once per process and again when the file changes.

The download reuses the ChatGPT app downloader (`chatgpt_app::HttpDownloader`): HTTPS only, at most five
redirects, resume, retry with backoff. Its failures are rewritten to say "LCU" rather than the app.

## Interface

The notification is `PreparationToast`, mounted in the application shell. `usePreparationStatus()` returns
the status, the ChatGPT status, the items still in progress or failed, `ready`, and `retry()`, so an action
that needs an item can show a "waiting for" step. In the browser preview, `?preparation=importing`,
`downloading-lcu`, `both`, `failed` or `ready` selects a fixture (combine with `&chatgpt=downloading`).

## Tests

Rust: `cargo test --locked preparation` (job fast path, joining, failure and retry, LCU verify, publish and
garbage collection with an injected downloader), the loopback download of unknown size in
`chatgpt_app`, and the fast-path ordering in `guest_image`. Frontend: `preparation.test.tsx` and
`preparation-toast.test.tsx`. These use fixtures and prove no live import, download or computer behavior.

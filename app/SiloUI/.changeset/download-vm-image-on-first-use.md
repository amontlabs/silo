---
"silo-ui": minor
---

Installers and updates are about 400 MB smaller: Silo no longer ships its VM image inside the app. It downloads the image (about 400 MB) once, in the background, before your first sandbox, checks it against a pinned checksum and reuses it across updates. Creating a sandbox waits for the download if it is still running, and a failed download offers Retry. Devices that already have the image keep using it.

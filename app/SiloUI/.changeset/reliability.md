---
"silo-ui": patch
---

- Quit no longer hangs if the window closes while Silo is saving settings.
- Settings that can't be read or come from a newer Silo can now be reset from the notice; the old file is kept beside it.
- Stopping a computer's runtime now asks it to shut down cleanly before forcing it.
- Several background checks (repository discovery, update checks, log cleanup, sound checks, launch) now recover from unexpected errors instead of staying stuck.
- A saved-settings directory sync failure no longer reports a successful save as failed.

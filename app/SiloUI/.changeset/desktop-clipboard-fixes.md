---
"silo-ui": patch
---

Fix desktop clipboard edge cases: Ctrl+Shift+C and Command shortcuts no longer reach the computer as extra modifiers, Paste and Copy report a disconnected or older desktop instead of pasting stale content, Copy waits for the new selection and says when it is too large, browser selections copy as plain text, holding the Linux shortcut starts one transfer, and an invalid desktop receipt keeps showing "Update desktop".

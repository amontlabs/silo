---
"silo-ui": patch
---

Fix desktop viewer edge cases: a desktop page that keeps reloading no longer piles up sound checks, a file dropped on one desktop window is uploaded only by that window, an upload that cannot be confirmed in the computer is reported as failed instead of "Uploaded", Copy no longer returns the computer's old selection on a fresh connection, holding Ctrl+Shift+V and releasing the modifiers first no longer types into the computer, and Quit lets a running file transfer clean up before stopping computers.

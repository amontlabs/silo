# How the Mac app is built

Decided on 16 November 2026.

The same decision says that macOS computers are driven from Swift directly, never through Rust
bindings: that half is `macos-vm-driver`, in the engine. "The app is views only" is a Decided line
of the design record's principles for what you see.

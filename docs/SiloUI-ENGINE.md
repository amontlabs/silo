# Silo engine extraction

The Cargo workspace is rooted at `app/SiloUI/src-tauri/Cargo.toml`. The Tauri
application remains the `silo-ui` package; the Tauri-free library is the
workspace member `app/SiloUI/src-tauri/engine` (`silo-engine`). This keeps the
existing Tauri build and lockfile while allowing `silod` to depend on the engine
without depending on Tauri.

`silo-engine` owns channel identity and defines `EngineContext`, `EnginePaths`,
`EngineIdentity`, `Event`, `Audience`, `MainThread` and `ProcessControl`. The Silo
UI's `TauriEngineContext` supplies those capabilities. Backend module groups
move into the library incrementally; commands remain in `silo-ui` as adapters
until their group is extracted.

## Current extraction state

The build channel, runtime operation gate (including its process-wide
`OPERATIONS` instance), and image-cache repair logic now live in `silo-engine`.
`silo-ui` re-exports the gate and image-cache functions for backend modules that
have not moved yet. Both workspace packages are Cargo's default test members, so
`cargo test --locked` covers the engine tests.

## macOS VM window host seam

The macOS VM host must not depend on a Tauri webview. When the VZ display code is
extracted, keep VM creation and `VZVirtualMachineView` attachment in the engine,
and expose the attach operation through a host seam that accepts the native
`NSWindow` supplied by the process hosting the engine. The operation must use
`EngineContext::main_thread()` for AppKit work. `silod` can then supply its own
window while Fjord and Silo UI hosts retain their own window policy. The current
window creation remains in `silo-ui` until that extraction step.

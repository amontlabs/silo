# macOS guests: engine selection

Research, 2026-10-09. Source review, crate inspection, Apple's license text and
a signed probe on this device (Apple Silicon, macOS 26.5). The resulting
feature is described in [macOS computers](../SiloUI-MACOS-COMPUTERS.md); the
direction is in [viewer direction](../SiloUI-DESKTOP-VIEWER-DIRECTION.md#revision-2026-10-09-native-local-display).

## Requirement

Run macOS computers on Apple Silicon and show them in a native
`VZVirtualMachineView` in a Silo window, managed by Silo's Rust backend.
libkrun cannot boot macOS, so this is a second engine next to MicroSandbox.
On Apple Silicon only Apple's Virtualization.framework can run macOS guests.
Every candidate below is a wrapper around it.

## Candidates

The owner set the candidates: Lume and the framework used directly from Rust.
[Tart](https://github.com/openai/tart) is excluded: since 2026-06-05 it is
licensed FSL-1.1-ALv2, a source-available license with a competing-use
restriction. Its public image templates are read only as a reference for the
keystroke sequences that drive Setup Assistant and Recovery.


| Option | License | Shape | Fit |
| --- | --- | --- | --- |
| [Lume](https://github.com/trycua/cua/tree/main/libs/lume) 0.6.1 (2026-10-04) | MIT | Swift CLI, HTTP API and MCP server. The VM runs inside the `lume` process; display through its own VNC service (optional since 0.6.0) | VM lives in another process; Silo could only show VNC. Telemetry on by default. Useful reference for unattended Setup Assistant presets |
| [objc2-virtualization](https://crates.io/crates/objc2-virtualization) 0.3.2 | Zlib OR Apache-2.0 OR MIT | Generated Rust bindings for the whole framework, from the objc2 project Silo already uses (objc2 0.6, objc2-foundation/app-kit 0.3) | VM, installer and `VZVirtualMachineView` in Silo's own process and window |
| UTM, VirtualBuddy (references only) | Apache-2.0, BSD-2-Clause | Complete GUI apps | Not embeddable; VirtualBuddy is a good reference for restore-image handling |
| [Code-Hex/vz](https://github.com/Code-Hex/vz) | MIT | Go bindings | Wrong language for Silo's backend |

## Decision

Use Virtualization.framework directly from Rust through `objc2-virtualization`.
The framework is the maintained upstream component; the crate is a thin
generated binding from a project Silo already depends on. Everything the
slice needs is bound: `VZMacOSRestoreImage` (catalog lookup and loading),
`VZMacOSInstaller`, `VZMacAuxiliaryStorage`, `VZMacHardwareModel`,
`VZMacMachineIdentifier`, `VZVirtualMachine` (start, stop, save and restore
state) and `VZVirtualMachineView`.

Lume does not meet the display requirement: a `VZVirtualMachineView` must be
in the process that owns the `VZVirtualMachine`, and Lume keeps the VM in its
own process, offering VNC. Using either would be the
streamed path the [direction revision](../SiloUI-DESKTOP-VIEWER-DIRECTION.md#revision-2026-10-09-native-local-display)
moves away from, plus a second process lifecycle to supervise.

Silo code stays limited to product policy: where files live, validation,
lifecycle states, and the window that hosts the view. Installation,
virtualization, display and input are Apple's.

## Framework constraints found

- **Entitlement.** `com.apple.security.virtualization` is required. An ad-hoc
  signature carrying it works: a probe signed with `codesign -s -` loaded the
  restore-image catalog; the same probe unsigned failed with "The restore image
  catalog failed to load". `tauri dev` runs an unsigned binary, so macOS
  computers need a signed bundle (`desktop:build:debug`).
- **Host and guest versions.** The host must support the guest's hardware
  model. On 2026-10-09 Apple's public catalog offered only macOS 27.0.1, while
  `fetchLatestSupported` on this macOS 26.5 device returned macOS 26.6.2
  (25G83, 19.8 GB). Silo therefore asks the framework rather than reading the
  catalog itself.
- **Two macOS guests at once.** The framework refuses a third running macOS
  guest (`VZErrorVirtualMachineLimitExceeded`). Linux guests do not count.
- **Process and thread.** The VM, its installer and its view belong to the main
  queue of the Silo process. Quitting Silo stops its macOS computers, matching
  local Linux computers. A crash of Silo stops them abruptly.
- **Save and restore state** (macOS 14+) exists but is tied to this device, needs
  the paused VM and a configuration that passes
  `validateSaveRestoreSupportWithError`. Not used yet.
- **Networking.** NAT needs no extra entitlement; bridged networking needs the
  restricted `com.apple.vm.networking` entitlement.

## Apple's license terms

From the [macOS Tahoe 26 software license agreement](https://www.apple.com/legal/sla/docs/macOSTahoe.pdf),
section 2.B.iii, paraphrased: a licensee who obtained macOS from the Mac App
Store or by automatic download may run up to two additional copies of macOS in
virtual machines on each Apple-branded computer they own or control that
already runs macOS. The permitted purposes are software development, testing
during development, using macOS Server, and personal non-commercial use. The
grant excludes using the virtual copies for "service bureau, time-sharing,
terminal sharing, relay service" and similar services. Section 3D allows a
lessor of Mac hardware only a single virtual instance, as a provisioning tool.
The single-copy license in section 2A (preinstalled software) has no
virtualization grant.

For Silo this means:

- Silo downloads macOS from Apple's servers on the user's own Mac; it does not
  redistribute macOS. The user is the licensee.
- The creation form states the license terms. Silo does not cap how many macOS
  computers a user creates; complying with the license is the user's
  responsibility. The framework separately refuses a third *running* macOS
  guest, and Silo reports that limit with a clear message.
- Offering macOS computers on another device to other people, or as a hosted
  service, falls under the excluded uses. Remote macOS computers are not part
  of this slice (see [macOS computers](../SiloUI-MACOS-COMPUTERS.md#not-covered-yet)).

This is a reading of the license for product design, not legal advice.

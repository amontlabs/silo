# The VM engine for Linux computers

Research of 19 November 2026. Every fact is dated by its source; facts change, so check the date
before relying on one. The design's summary and what is still open are in
[DESIGN.md, section 9.10](../plan/DESIGN.md#910-the-vm-engine-for-linux-computers-research-19-november-2026).

The question: which engine runs Silo's Linux computers, on a Mac now and on a Linux host later.
The candidates that matter are MicroSandbox, libkrun (which MicroSandbox is built on), Apple's
virtualization framework, and, for Linux hosts only, Firecracker and Cloud Hypervisor.

## 1. What Silo needs from it

From the design (sections 3, 6 and 9):

- Every computer's traffic goes through Silo's own network stack, never a built-in NAT or a
  built-in policy layer.
- Disk checkpoints and forks that share blocks; memory checkpoints and forks of a running computer
  later.
- Memory given back to the host when a computer is idle.
- One helper process per computer, so that the engine can restart without stopping computers.
- A full Ubuntu with systemd and Docker inside.
- A Linux desktop that an agent drives and the user watches in a native window.
- Fast work folders, measured before adoption (an 8x slowdown on parallel file work is the
  failure to avoid).
- An interface the engine, written in Zig, can link through C.
- The same engine on a Mac (Apple Silicon, arm64 guests) and on a Linux host (Decided:
  `engine-on-linux`), and later on a shared server or a cloud (Decided: `runs-anywhere`).

What Silo does not need from an engine, because the design builds it: a network policy layer,
secret substitution, an OCI image pipeline, a catalog database, SDKs, a cloud.

## 2. MicroSandbox

[superradcompany/microsandbox](https://github.com/superradcompany/microsandbox), Apache-2.0,
Rust. Each sandbox is a libkrun VM run by the company's own copy of libkrun,
[superradcompany/libkrun](https://github.com/superradcompany/libkrun), published as `msb_krun`.

**Activity and team** (GitHub API, 19 November 2026)

- 9,136 stars (8,620 on 9 October), 80 contributors, 112 open issues.
- Nine releases from v0.7.8 (9 October) to v0.7.16 (18 November). Still pre-1.0.
- 122 commits on `main` from 10 October to 19 November: the two founders 73, one new regular
  committer 17, the rest from occasional contributors. The GitHub organisation lists two members.
- Of the newest 100 issues (8 October to 19 November), 97 were opened by people outside the core
  team, and a core maintainer commented on 26 of them (median first comment 5.4 hours). The other
  71 had no maintainer comment.
- npm downloads: 700,716 in September, 897,442 in October
  ([npm API](https://api.npmjs.org/downloads/range/2026-10-01:2026-10-31/microsandbox)).
- The hosted cloud is still a private beta. No blog post since 1 June 2026.

**Security advisories**
([list](https://github.com/superradcompany/microsandbox/security/advisories)): 14 in total, 10
published since September, 2 critical. They sit in the volume and copy API, snapshot import and
capture, the root-disk journal, the catalog database and the network policy layer. None is a
VM escape.

**What it offers Silo**

- Memory snapshots and live forks of a running VM with copy-on-write memory, on macOS, Linux and
  Windows, since v0.7.0 (16 September 2026). This is its main advantage: no other open-source
  engine found offers it for Linux guests on a Mac.
- A detached VM survives the program that created it and can be reattached by name
  (`docs/sandboxes/lifecycle.mdx`).
- It boots a raw or qcow2 disk image, so a full Ubuntu is possible; systemd as the first process
  works with `--init auto`, with open bug
  [#1676](https://github.com/superradcompany/microsandbox/issues/1676).

**Where it falls short for Silo**

- **No way to plug in Silo's network stack.** Its network stack (smoltcp) is built into the
  runtime; no SDK or C option takes a socket or raw frames.
- **Memory is not given back on a Mac.** Open issue
  [#1938](https://github.com/superradcompany/microsandbox/issues/1938) (25 October): free page
  reporting does nothing on macOS. Upstream libkrun fixed this on 20 October
  ([libkrun#931](https://github.com/libkrun/libkrun/pull/931)); `msb_krun` has not taken it.
- **Its copy of libkrun drifts.** 146 commits ahead of upstream and 653 behind, with no common
  commit since 27 February 2026. No upstreaming pull requests from the company were found.
- **No stable C interface.** The only one is the Go SDK's FFI layer (`microsandbox_go_ffi.h`,
  about 200 functions), shaped for Go and not promised stable.
- **No screen yet.** A maintainer says virtio-gpu is planned for 0.8.x
  ([#1805](https://github.com/superradcompany/microsandbox/issues/1805)).
- **Silo already pays for it.** The old Silo carried 24 patches and about 16,600 lines on top of
  MicroSandbox, and each upgrade brought regressions (plan notes, tag `plan-2026-11-18`,
  `vm-runtime`). Most of the pull requests sent upstream fix its snapshot store, disk layers and
  compaction, which are the parts Silo's design would replace.

**How its snapshots are built.** In two layers:

- In `msb_krun`: capture and restore of every virtual device, the processors and memory, per
  platform (`src/vmm/src/{device_state,memory_state,execution_state,memory_capture}.rs`,
  `src/vmm/src/{macos,linux,windows}/vstate.rs`). It is a Rust interface; the C header exports no
  snapshot function. In all, the copy adds about 65,000 lines to the upstream commit it started from, tests included.
- In MicroSandbox itself: about 64,000 lines of checkpoint and snapshot code in the runtime and
  the SDK, tied to its disk layout (EROFS layers in a VMDK) and its catalog database.

## 3. libkrun

[libkrun/libkrun](https://github.com/libkrun/libkrun) (formerly `containers/libkrun`),
Apache-2.0, Rust with a generated C interface. Led by Sergio Lopez (Red Hat). Runs on KVM (Linux),
HVF (macOS on Apple Silicon) and WHP (Windows).

**Activity** (GitHub API, 19 November 2026)

- 2,930 stars. Releases v1.19.6 (29 September), v1.19.7 (22 October), v1.19.8 (6 November).
- More than 100 commits since 1 October. Top contributors by commits: 732, 292, 174, 163.
- `main` is libkrun 2.0, which breaks the 1.x interface; production use is told to take the
  newest `stable-*` branch. The new native Rust interface with a generated C interface was merged
  on 11 September ([#718](https://github.com/libkrun/libkrun/pull/718)). On 1 September a
  maintainer expected 2.0 in about four weeks; no 2.0 tag exists on 19 November.

**Against Silo's needs**

- **Own network stack: yes.** `krun_net_device_new_unixgram_fd` and its stream and path variants
  connect the VM's network card to a socket, with raw frames
  ([`include/libkrun.h`](https://github.com/libkrun/libkrun/blob/main/include/libkrun.h)). TSI,
  the mode that bypasses a network stack, is a flag on the vsock device and can be left off.
- **Memory given back: yes, on a Mac too.** Free page reporting; since v1.19.8, reclaimed memory
  is returned to macOS.
- **One process per VM: yes.** libkrun runs the VM in the process that calls it, which is the
  design's helper per computer.
- **Disk images:** raw, qcow2, VMDK. Forks that share blocks are Silo's to build (APFS clones on
  a Mac, reflinks on Linux).
- **Memory snapshots and forks: not yet.** Upstream has none. An outside contributor's pull
  request ([#762](https://github.com/libkrun/libkrun/pull/762), about 3,200 lines) saves and
  restores a running VM on HVF; lazy copy-on-write restore is planned
  ([RFC #748](https://github.com/libkrun/libkrun/issues/748)). The maintainers want snapshots for
  2.x and, on 1 September, put the review off until after 2.0, then expected in about four
  weeks. On 19 November:
  - 2.0 is not out, and no newer date was found.
  - The pull request's last commit is from 15 July and its last activity from 24 September. It
    may need rework against the new interface.
  - Restore loads all of memory up front. Copy-on-write restore, which forking needs, is planned
    but not written.
  - Nothing exists for KVM: one person offered on 24 September to start it.
  - No date has been given by anyone. Getting it sooner means doing the work.
- **Screen: the plumbing, not the window.** A virtio-gpu device (with Venus for GPU drawing), a
  display interface the host program implements (configure a screen, receive each frame and its
  changed area), and an input interface the host program feeds
  ([`src/display/libkrun_display.h`](https://github.com/libkrun/libkrun/blob/main/src/display/libkrun_display.h)).
  The only viewer upstream is a GTK example for Linux. For Silo, the Swift app would draw the
  frames.
- **Full Ubuntu with systemd:** not established by this research.
- **Work folders:** virtio-fs with a DAX window and cache options. No speed numbers found.

**A screen on a Mac: what exists outside upstream.**
[fxgl/steamac](https://github.com/fxgl/steamac) (Apache-2.0, started 4 October 2026) runs
SteamOS on libkrun on Apple Silicon and carries
[patches](https://github.com/fxgl/steamac/tree/main/host/libkrun/patches) against upstream.
Against what Silo's issue [#1805](https://github.com/superradcompany/microsandbox/issues/1805)
asks for:

| Wanted | Upstream libkrun, 19 November 2026 |
|---|---|
| The screen in a window, with keyboard and mouse | The interfaces exist; the Mac window is Silo's to write |
| A hardware cursor | No: the display interface has none |
| Resolution follows the window | No: steamac patch 0012 |
| Sound | Upstream removed its built-in sound device for a vhost-user one; steamac's CoreAudio backend (0014) is not upstream |
| GPU frames shown directly | Not upstream: steamac patches 0007 to 0009 |

steamac's input fixes (0004, 0005) and the scanout fix (0006) are upstream since 28 September.

## 4. Apple's virtualization framework, for Linux guests

Closed, Apple. Silo uses it for macOS computers (Decided).

- **Own network stack: yes.** `VZFileHandleNetworkDeviceAttachment` carries raw frames over a
  datagram socket
  ([docs](https://developer.apple.com/documentation/virtualization/vzfilehandlenetworkdeviceattachment)).
- **No count limit for Linux guests.** The two-VM limit applies to macOS guests only (Apple
  developer support, [September 2026](https://developer.apple.com/forums/thread/845455)).
- **Memory save and restore:** macOS 14 and later. Restore needs a matching configuration and the
  same machine identifier, and a file from a newer macOS is refused
  ([forum](https://developer.apple.com/forums/thread/782007)). Tart shipped suspend and resume for
  Linux VMs on 17 November 2026 ([PR](https://github.com/openai/tart/pull/1297)). Restoring one
  saved state as a second, separate VM: no evidence found. No live fork.
- **Memory given back:** a traditional balloon only. Apple's `container` documentation says freed
  guest memory is not returned to the host
  ([technical overview](https://github.com/apple/container/blob/main/docs/technical-overview.md)).
- **Screen:** native, `VZVirtioGraphicsDeviceConfiguration` shown in `VZVirtualMachineView`.
- **macOS 27** (WWDC 2026, [session 224](https://developer.apple.com/videos/play/wwdc2026/224/)):
  DiskImageKit with copy-on-write overlay layers, USB passthrough, Secure Boot for Linux.
- **Runs only on a Mac**, so a Linux host would need a second engine.

## 5. For Linux hosts and a cloud only

- **Firecracker** (Apache-2.0, AWS; v1.18.0 on 9 November 2026): full and diff snapshots,
  copy-on-write memory from the snapshot file, userfaultfd restore, free page reporting
  ([snapshots](https://github.com/firecracker-microvm/firecracker/blob/main/docs/snapshotting/snapshot-support.md)).
  E2B, Fly and Vercel run on it. No macOS.
- **Cloud Hypervisor** (Apache-2.0 and BSD-3; v54.1 on 11 November 2026): snapshot and restore
  with copy, on-demand and copy-on-write memory, live migration
  ([docs](https://github.com/cloud-hypervisor/cloud-hypervisor/blob/main/docs/snapshot_restore.md)).
  Modal's VM sandboxes run on it. No macOS.

Either would mean a second engine beside the one on the Mac.

## 6. Others considered

- **Apple `container` and Containerization** (Apache-2.0): Apple's own NAT-style network,
  memory not returned, no checkpoints, Swift only.
- **smolvm** (Apache-2.0, libkrun): claims memory snapshots and copy-on-write branches; one
  developer has 2,042 of its commits.
- **Docker Sandboxes:** closed, a command-line tool, not embeddable.
- **Tart:** now under the `openai` organisation on GitHub, under FSL-1.1-ALv2, which bars
  competing use.
- **OrbStack:** closed, paid for business use.

## 7. Building on libkrun with Silo's own patches

What it would take, from the facts above:

- **Silo builds** disk checkpoints and forks, a small guest agent or SSH, the network stack (planned
  anyway), and the Mac window for the screen.
- **Silo gives up for a while** memory snapshots and forks of a running computer, until they land
  upstream or Silo carries them. The design already starts Linux checkpoints with the disk only
  (9.7).
- **This is MicroSandbox's strongest point.** It has memory snapshots and forks of a running
  computer working today, on a Mac and on Linux. With libkrun, Silo either goes without them for
  an unknown time or builds them.
- **Two ways to get memory snapshots.** Port them from `msb_krun` into Silo's own copy, or help
  [#762](https://github.com/libkrun/libkrun/pull/762) land upstream. The first means carrying
  thousands of lines of device and memory state code against an upstream in the middle of its 2.0
  rewrite, the way `msb_krun` fell 653 commits behind. The second leaves nothing to carry once
  merged. Both copies are Apache-2.0, so `msb_krun`'s code can serve as a reference with credit.
- **Patches to the engine are boundary code.** Snapshot, device and display code runs in the
  helper process, which is what a hostile guest attacks: guest-controlled state is read back on
  restore. Most of MicroSandbox's 2026 advisories were in exactly these convenience layers
  (snapshot capture and import, the disk journal, file copy).

## 8. To measure before deciding

1. A Zig helper boots the Ubuntu image on libkrun through its C interface, with traffic going to
   a stub network stack over a socket, on a Mac and on the Linux test machine.
2. How long a fork takes from a cloned disk with a cold boot.
3. Whether systemd and Docker run in that image.
4. How much memory an idle computer gives back on a Mac.
5. Work folder speed against the 8x budget.

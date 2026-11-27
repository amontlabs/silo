# Step 2 under B: a Linux main runs

Part of option B's specs ([README](README.md)). It takes the place of #370's
`2-main-runs.md` and builds `computers-main-on-a-mac` in its Linux form: the new feature
`computers-linux-main-on-a-mac` (this PR), with `engine-matches-wanted`,
`agent-programs-run-unmodified`, `agent-programs-reach-every-computer` and
`computers-stopped-and-paused`. It needs step 1, which is #370's `1-engine.md` unchanged.
The formats are #370's `formats.md`, with the changes listed at the end of this page.

## What you can do when it is done

```
$ silo setup
This Mac: Apple M4 Pro, macOS 26.5, 612 GB free. Main needs about 40 GB.

Main is a Linux computer: Ubuntu 26.04 for arm64, with a kernel Silo builds.
Xcode and macOS apps do not run in it.

Main's network. Open reaches the internet. It does not reach your local network or this Mac.
Silo reads the traffic between Claude Code and Codex and their providers, to count spend;
nothing else an agent sends is checked or held yet. Open it? [y/N] y

Main will get, each from its vendor's own installer:
  git, tmux, build-essential, Node.js 24, Claude Code, Codex
Remove any? (enter to keep all)

To reach main by name, Silo adds this line at the top of ~/.ssh/config:
  Include "~/Library/Application Support/Silo/ssh/config"
Add it? [y/N] y

Downloading Ubuntu 26.04 (943 MB)    ████████████░░  86%
...
Main is ready. `ssh main` opens a shell, `ssh main.keep` a tmux session that outlives the terminal.

$ ssh main.keep
polarzero@main:~$ claude --dangerously-skip-permissions
```

```
$ silo status
Engine    running, history 5b1e…, event 214
Recorder  running, holds every event up to 214
main      running since 21:14   Linux, boot 1   network open, model traffic read, nothing else checked
          way in: ssh main, ssh main.keep
```

What differs from #370's screen, and why:

- **No macOS version is asked for.** libkrun runs on Apple's Hypervisor framework, which Podman's
  libkrun machines use from macOS 14. polarzero's Mac ran 26.5 on 21 November. The first run
  refuses only an Intel Mac. The kit's test L1 confirms the oldest macOS on which the helper boots
  main; the spec assumes 14.
- **About 40 GB, not 110 GB.** The image is 943 MB to download and main's disk is a 32 GiB sparse
  file. The rest is the floor of free space of step 4.
- **Open does not reach your network or your Mac.** Main's network goes through gvproxy and the
  boundary from its first start (`network-stack`, and #373's change to step 2). Apple's NAT is
  not available to a libkrun computer, so the phase of #370 on Apple's NAT does not exist here.

## The pieces

| Piece | Where | What it does |
|---|---|---|
| The core's computer rows | `core/computers.zig`, `operations.zig`, `images.zig` | As #370. `system` holds `linux-arm64`, a value the core only compares (`neutral-core`) |
| The helper | `engine/linux-vm/`, Zig, one process per computer | Runs the virtual machine in its own process through libkrun. Starts, stops, pauses it. Opens SSH connections into it over vsock |
| The kernel | `engine/data/kernel/`, built in CI | Silo's own Linux kernel for arm64, pinned, with Silo's configuration. It comes from outside main's disk |
| The image driver | `drivers/image.zig` | Downloads Ubuntu's cloud image, checks it by hash, converts it to a raw disk |
| The catalog driver | `drivers/catalog.zig` | As #370: makes `computers/c1/live/` from a clone of the image's raw disk |
| The seed | `drivers/seed.zig` | Writes the first boot's `cidata` disk: the user, Silo's SSH key, the way in |
| The helper driver, the setup driver, the SSH entries driver | as #370 | Unchanged, apart from the steps of the first run below |

### The helper

One Zig program, `silo-helper`, signed with `com.apple.security.hypervisor`, the entitlement
libkrun needs on a Mac. It links libkrun as a dynamic library that Silo builds, pins and signs
(`libkrun-patches`), with Silo's patches applied. libkrunfw is not used: the kernel is a file
Silo ships, loaded with libkrun's external kernel payload.

The helper is written against libkrun's 2.0 interface (`include/libkrun.h` on libkrun's `main`,
read on 25 November 2026), pinned to a commit until 2.0 is tagged. The 1.19 interface has no
pause and no resume, which `pause-every-computer` needs. The kit's launcher uses 1.19, which is
what can be installed today, and needs neither.

| Command | Started by | Does |
|---|---|---|
| `silo-helper run c1` | launchd, job `com.amontlabs.silo.helper.c1` | Builds the virtual machine from `computers/c1/live/`, listens on `run/helpers/c1.sock`, waits for the engine's `start`, then runs the machine in this process |
| `silo-helper fetch` | the image driver | Downloads the pinned image to `ubuntu.img.part`, resuming from its length. Prints progress as JSON lines |
| `silo-helper convert` | the image driver | Reads the qcow2 file and writes `disk.raw`, then checks the raw file's hash |

`run` builds the machine with these calls, in order:

| Call | With |
|---|---|
| `krun_payload_load_external` | `kernel/Image`, raw format, no initramfs, the command line below |
| `krun_vmm_builder_vcpus`, `krun_vmm_builder_ram_mib` | half the Mac's performance cores, at least 4; half its memory, at most 16 GiB (`sizing-target`) |
| `krun_block_device_new` | `live/disk.raw`, raw, read-write, sync mode `full` (flushes reach the drive), id `root` |
| `krun_block_device_new` | `seed.iso`, read-only, only on the first start |
| `krun_vsock_device_new` with no TSI | then `krun_vsock_device_add_unix_port(22, run/c1/ssh.sock, listen=true)` and `krun_vsock_device_add_unix_port(1024, run/c1/probe.sock, listen=false)` for step 7 |
| `krun_net_device_new_unixgram_path` | gvproxy's datagram socket, with the vfkit flag, a MAC address made with the computer |
| `krun_balloon_device_new` with free page reporting | memory main frees goes back to the Mac (`linux-vm-engine`, its notes) |
| `krun_rng_device_new` | entropy for the first boot's keys |
| `krun_vmm_builder_shutdown_support(true)` | the orderly stop below, on a Mac |

No `krun_fs_device_*` is added: nothing of the Mac is shared into main (`device-not-shared`). A
test reads the built configuration back through `silo-helper run --print-config` and refuses any
filesystem, display or input device.

The kernel's command line is fixed by Silo, and main cannot change it:

```
root=/dev/vda1 rootfstype=ext4 rw console=hvc0 init=/lib/systemd/systemd systemd.machine_id=<id> lsm=landlock,lockdown,yama,bpf lockdown=integrity
```

The root is named `/dev/vda1`, not by its label: resolving a label needs an initramfs, which Silo
does not ship. Ubuntu's 26.04 image keeps its root filesystem in partition 1 (`cloudimg-rootfs`), `/boot` in
partition 13 and the EFI partition in 15, read from the image on 25 November 2026. The image
driver checks partition 1's type and label when it converts the image. `/boot` and Ubuntu's own
kernel in it are never used.

The machine runs in the helper's own process, as `processes` wants: libkrun's `krun_vmm_run`
takes the calling thread, and the helper's socket is served by a second thread started before it.
The helper is a launchd job of its own, as in #370, so a crash of the engine never stops main.

### The kernel

Silo builds main's kernel. It is not the one on main's disk, so an agent with root in main cannot
replace it: Ubuntu's own kernel in `/boot` is never booted. This is what
`processes-from-inside-linux` relies on ("The computer's kernel comes from outside its disk").

- **Source.** The kernel.org long-term branch libkrunfw uses (6.12 on 25 November 2026), pinned to
  a release and its hash.
- **Configuration.** `engine/data/kernel/silo-arm64.config`, a fragment over `defconfig`, kept
  short and read in review:

| Group | Options | Why |
|---|---|---|
| Devices | `VIRTIO_BLK`, `VIRTIO_NET`, `VIRTIO_VSOCKETS`, `VIRTIO_BALLOON`, `HW_RANDOM_VIRTIO`, `VIRTIO_CONSOLE`, `EXT4_FS`, `GPIO_PL061`, `KEYBOARD_GPIO` | Built in, so no initramfs is needed. The last two carry libkrun's stop request on a Mac |
| No loading | `MODULES` off, `KEXEC` off, `DEVMEM` off, `STRICT_DEVMEM`, `SECURITY_LOCKDOWN_LSM` | Root in main cannot add code to the kernel or replace it while it runs |
| Containers | `OVERLAY_FS`, `CGROUPS` and every controller, `NAMESPACES`, `VETH`, `BRIDGE`, `NETFILTER` with `NF_TABLES` and the `xt_` matches Docker uses, `BRIDGE_NETFILTER` | Docker runs (`containers-on-macos` says a Linux computer is where containers run) |
| The probe (step 7) | `BPF_SYSCALL`, `BPF_JIT`, `BPF_EVENTS`, `TRACEPOINTS`, `DEBUG_INFO_BTF`, `FANOTIFY`, `PROC_EVENTS`, `CGROUP_BPF` | What the stock libkrunfw kernel lacks (#351) |
| Off | `KPROBES`, `FTRACE` beyond tracepoints, `AUDIT` | Nothing in the probe needs them |

- **Built** in CI with a pinned compiler, reproducibly (`KBUILD_BUILD_TIMESTAMP` fixed), and
  shipped with Silo as `kernel/Image` with its hash in `engine/data/kernel.zon`. The helper checks
  the hash before it boots.
- **Modules from Ubuntu** are absent from main. Every driver main needs is built in. `modprobe`
  inside main fails, and nothing in Ubuntu's boot needs it to succeed; the kit's test K01 checks
  that `systemctl is-system-running` says `running`, not `degraded`.

### The image

`computer-image` says Silo keeps the installed system before its first start. Under B that is the
raw disk converted from Ubuntu's cloud image, never booted:

- **Source.** `ubuntu-26.04-server-cloudimg-arm64.img` from `cloud-images.ubuntu.com/releases/`,
  a qcow2 file of 943 MB on 25 November 2026, pinned by the SHA-256 Ubuntu publishes in
  `SHA256SUMS` and by the hash of the raw disk it converts to. Both hashes are in `engine/data/images.zon`, which a Silo
  release updates. A download whose hash differs is deleted, and the first run says so.
- **Conversion.** The helper's `convert` reads qcow2 version 3 with deflate compression, no
  encryption, no snapshots and no backing file, and refuses anything else. Ubuntu's file is
  exactly that, with 95 % of its clusters compressed; a trial converter read it in 8.6 s
  ([proto/results.txt](proto/results.txt)). It writes
  `images/ubuntu-26.04-<serial>/disk.raw` sparse, grows it to 32 GiB, and checks the raw hash
  before the growth. The growth is a larger file; the partition and the filesystem grow on the
  first boot (`cloud-init`'s `growpart`).
- **No macOS restore image** and no `aux.img`, `hardware-model.bin` or `machine-id.bin`. Main's
  folder holds `disk.raw`, `config.json` and `machine-id`, a random 128-bit value made with the
  computer and passed on the command line, so a fork made later gets a new one
  (`fork-identity`).

### The first boot's seed

Ubuntu's cloud image sets itself up at first boot with cloud-init, from a small disk labelled
`cidata`. Lima and Multipass set up their Linux machines the same way. The seed driver writes
`computers/c1/seed.iso`, an ISO 9660 image of about 64 KB with two files:

```yaml
# user-data
#cloud-config
users:
  - name: polarzero            # your Mac's login name
    gecos: polarzero
    shell: /bin/bash
    lock_passwd: true          # no password exists; the way in is Silo's key
    sudo: "ALL=(ALL) NOPASSWD:ALL"
    ssh_authorized_keys: ["ssh-ed25519 AAAA… silo c1"]
ssh_pwauth: false
disable_root: true
growpart: {mode: auto, devices: ["/"]}
write_files:
  - path: /etc/systemd/system/sshd-vsock.socket
    content: |
      [Socket]
      ListenStream=vsock::22
      Accept=yes
      [Install]
      WantedBy=sockets.target
  - path: /etc/systemd/system/sshd-vsock@.service
    content: |
      [Service]
      ExecStart=-/usr/sbin/sshd -i
      StandardInput=socket
  - path: /etc/systemd/logind.conf.d/silo.conf
    content: |
      [Login]
      HandleRebootKey=poweroff
runcmd:
  - [systemctl, enable, --now, sshd-vsock.socket]
  - [touch, /etc/cloud/cloud-init.disabled]
```

```yaml
# meta-data
instance-id: c1-<machine-id>
local-hostname: main
```

- The seed is attached on the first start only, read-only. `cloud-init.disabled` stops cloud-init
  on every later boot, so a seed disk can never set main up again, and nothing from outside
  changes main after its first boot.
- Ubuntu's systemd may already listen for SSH on vsock in a virtual machine
  (`systemd-ssh-generator`, from systemd 256). The unit above makes it explicit, so the way in
  does not depend on a generator's condition. The kit's test K03 checks both.
- The host key that main makes at first boot is read over the console log
  (`console=hvc0`, the lines cloud-init prints) and pinned in Silo's `known_hosts` before the first
  SSH connection.

### The way in

The SSH entries and `silo proxy` are #370's, unchanged. What changes is how the helper reaches
main's SSH server: over vsock, not over the network.

- libkrun maps vsock port 22 of main to the Unix socket `run/c1/ssh.sock`, which the helper's
  own process listens on (`listen=true`: the Mac side opens each connection).
- `silo proxy c1` sends `{"connect":{"port":22}}` on `run/helpers/c1.sock`. The helper connects to
  `run/c1/ssh.sock`, writes a `connection` line, and passes the socket back with `SCM_RIGHTS`.
- The way in does not use main's network. So **a closed main keeps its SSH**, from the first build,
  which #370 could offer only once gvproxy replaced Apple's NAT. And a restart of gvproxy or of the
  boundary never cuts an SSH session.
- No port of the Mac listens. A process of the Mac that is not yours cannot open the socket: it is
  under `<data>`, mode 0600, in folders of mode 0700.

### The network

From the first start, as `network-stack` answers and as #373 asks of step 2:

```
main's virtio-net ── unixgram ── gvproxy (patched) ── the boundary ── the internet
```

- The helper starts gvproxy as its child with `-listen-vfkit unixgram://<data>/run/c1/net.sock`,
  the mode Podman uses with libkrun on a Mac. gvproxy is #363's patched build: no HTTP API, no map
  of `192.168.127.254` to the Mac's `127.0.0.1`, no route to the Mac's local network. Stream 7 of
  25 November makes it a maintained component.
- gvproxy hands TCP connections to the boundary, as #373's step 6 says. A closed main is gvproxy
  with no outbound connection; SSH is unaffected.
- What main reaches is the same as under A with gvproxy, and the hostile test is the same. The one
  Mac unknown of A here, #359's test 07 (gvproxy under a macOS computer), does not arise: gvproxy
  with a libkrun Linux computer is what Podman machine runs on a Mac.

### What the core holds for a computer

#370's table, with two changes:

| Field | Under B |
|---|---|
| `system` | `linux-arm64` |
| `place_refused` | gone: Apple's limit of two macOS computers does not apply to Linux computers (`linux-vm-engine`'s notes, Apple developer support, September 2026) |

The reconciling table is #370's without the `place_refused` row.

### The first run, as one operation

| Step | Effect | Kept when it stops | On failure |
|---|---|---|---|
| 1. Check the Mac | none; the request carries the facts `silo setup` read | | Not Apple silicon: `not_this_mac`. Too little disk: `disk_too_small` with how much is missing |
| 2. Reserve | write `reserve` (#370) | the file | |
| 3. Download | `silo-helper fetch` | `ubuntu.img.part` | resumes from its length; a wrong hash deletes it and stops with `image_hash_mismatch` |
| 4. Convert | `silo-helper convert` | `disk.raw` once its hash checks, renamed into place | removed and done again |
| 5. Make main | the catalog driver clones `disk.raw` into `computers/c1/live.new/`, grows it to 32 GiB, makes the SSH key, the machine id and `helper.key`, writes `seed.iso`, then swaps it into `live/` | | `live.new/` removed at the next start |
| 6. First start | load the helper's job, `start` with `seed` | | the console log is kept in `computers/c1/console.log` and shown |
| 7. SSH answers | `silo proxy` until the server answers, at most 3 minutes; the host key read from the console log first | | `first_boot_timeout`, with the last 40 lines of the console |
| 8. Check the seed took | over SSH: your user, no password, `cloud-init status` is `done`, `/etc/cloud/cloud-init.disabled` exists | | the step that failed, with cloud-init's log |
| 9. Install the list | one step per item, over SSH with the key, each with its vendor's documented command for Ubuntu | each item done | as #370 |
| 10. The authority | #373's new step, in its Linux form: `authority.crt` into `/usr/local/share/ca-certificates/silo.crt`, `update-ca-certificates`, and the variables of #373 in `/etc/environment` | | the program that refused, named |
| 11. Write the entries | the SSH entries driver | | |
| 12. Drop the seed | `seed.iso` deleted; the next start attaches no seed | | |

Against #370's first run, steps 3, 4, 6, 8 and 11 have no Mac unknown left:

- **No provisioning.** cloud-init has set up Ubuntu cloud images the same way for years; the
  FB23241619 report (provisioning fails when the user name is the Mac's) has no counterpart.
- **No Setup Assistant, no Keychain question, no Remote Login switch.** SSH is a systemd unit.
- **The authority goes into Linux's store,** which #347 and #369 measured: Claude Code, Codex and
  eight more programs accepted an authority there. #359's test 21 (the macOS Keychain over SSH)
  is not needed.
- **The list for Ubuntu** (`main-set-up-for-agents`): `git`, `tmux` and `build-essential` from
  Ubuntu's archive; Node.js 24 from NodeSource's documented script; Claude Code from Anthropic's
  installer; Codex from npm, as OpenAI documents. Nothing is pinned, as in #370.

### Stop, pause, and a restart of the Mac

- **Stop** (`what-stop-ends`): the helper calls `krun_vmm_handle_shutdown`, which on an arm64 Mac
  presses a key in main through libkrun's GPIO device. libkrun's header calls it a restart key, and
  systemd-logind reboots on that key by default, so the seed writes `HandleRebootKey=poweroff` in
  `/etc/systemd/logind.conf.d/silo.conf`. The kit's test L1 checks that main powers off. After
  30 seconds the helper exits, which ends the virtual machine like a power cut. The record says
  which ended it. On a Linux host, where libkrun's orderly shutdown is not available, the helper
  runs `systemctl poweroff` over its own SSH connection first; the kit uses that.
- **Pause** (`pause-every-computer`): `krun_vmm_handle_pause` and `krun_vmm_handle_resume`. A
  paused computer whose helper ends comes back stopped, as #370 says.
- **A restart of the Mac.** As #370. One difference may exist: #352 found Apple's Virtualization
  framework unsafe outside a login session, and whether the lower Hypervisor framework that libkrun
  uses has the same limit is not known. If it has not, the helpers could run as launch daemons and
  main could come back before you log in. The spec keeps them as user agents, as #370
  does, because `<data>` is in your home folder, which FileVault keeps locked until you log in.
  The kit's test L7 checks whether a helper runs logged out.

### The record of step 2

#370's, plus the console: every line main's kernel and systemd print on `hvc0` goes to
`computers/c1/console.log`, a ring of 4 MiB, and not into the record. It is main's own output,
not a fact; `silo status --console` prints its tail.

## What happens on a crash

| What stops | What happens to main | What the engine does |
|---|---|---|
| The engine | Nothing: the helper is its own launchd job | As #370 |
| The helper | Main stops with it, as a power cut. ext4's journal brings its disk back to a consistent state at the next boot | As #370: `helper-ended`, then started again |
| gvproxy | Main loses its network; SSH stays up | The helper starts gvproxy again on the same socket. A connection that was open is cut, and the boundary writes it |
| `silo-helper fetch`, `convert` | | Retried; the download resumes, the conversion starts over |
| The first boot, cut off | | The seed is attached again at the next start, and cloud-init finishes, since it has not written `cloud-init.disabled` yet |
| The Mac sleeps | Main is suspended with it. Its clock is set again by `systemd-timesyncd` through the boundary at wake | Step 3 records it |

## Cost targets

| Measure | Target |
|---|---|
| `silo setup` with nothing downloaded, at 300 Mbit/s, to `ssh main` | under 5 minutes; set from test L1 |
| `silo start` of main, to SSH answering | under 10 seconds; set from test K01 and L1 |
| `silo stop` of a busy main | 30 seconds at most, by construction |
| The helper's memory beyond main's own, idle | under 64 MB |
| Memory given back after main frees 4 GiB | at least 3 GiB within a minute, on a Mac (test L2) |

## Tests

| Test | Checks | Where |
|---|---|---|
| The simulator with a fake helper, crashes at each step of `setup` | As #370 | CI |
| Contract tests: the fake helper and `silo-helper` answer the same cases | The fake is honest | CI for the fake; a Linux host with KVM for the real helper, then the Mac |
| The configuration read back | No filesystem, display or input device; vsock without TSI | CI on a Linux host with KVM, then the Mac |
| The seed: `seed.iso` read with `isoinfo` and `cloud-init schema` | It is what the spec says; a user name with a quote or a newline is refused | CI |
| The qcow2 converter against `qemu-img convert` on Ubuntu's image, and fuzzed | Same bytes; refuses every other shape | CI |
| The kernel's configuration | `MODULES`, `KEXEC`, `DEVMEM` off; BTF present; Docker's options in | CI, from the built kernel's `/proc/config.gz` read in main |
| Kit K01 to K06 | Boot, systemd and Docker, SSH over vsock with and without network, gvproxy, stop and a cut, a cold fork | a Linux host with KVM |
| Kit K13, hostile, as root in main | main cannot load a module, replace the running kernel, reach `run/engine.sock`, the Mac's `127.0.0.1` or its local network; replacing `/boot/vmlinuz` changes nothing at the next boot | a Linux host with KVM, then the Mac |
| Kit L1, L2, L7 on the Mac | The helper boots main on HVF on the oldest macOS supported; memory given back; logged out | polarzero's Mac |
| #359's test 08, unchanged | Claude desktop and the Codex app reach main through the included entry with fd passing | polarzero's Mac |
| #359's test 02, in its Linux form | Claude Code and Codex stay signed in over SSH, after a reconnect and a restart of main | a Linux host with KVM, then the Mac |

## Done when

- a Mac with nothing of Silo on it reaches `ssh main` with Claude Code and Codex installed, and you
  answered only the three questions of `silo setup`;
- `silo setup` killed at any step and run again ends with the same main;
- `silo stop` ends main within 30 seconds and `silo start` brings it back;
- a closed main still answers `ssh main`;
- the hostile test passes, and `silo status` says what main's network reaches.

## What changes in #370's formats

- `<data>/images/ubuntu-26.04-<serial>/` holds `ubuntu.img.part`, then `disk.raw`. No
  `installed/`, no `restore.ipsw`.
- `<data>/computers/c1/live/` holds `disk.raw`, `config.json`, `machine-id`. A checkpoint holds
  `disk.raw` and `meta` (and `writes`, step 4).
- `<data>/computers/c1/` gains `seed.iso` until step 12 of the first run, and `console.log`.
- `<data>/run/c1/` holds `ssh.sock`, `net.sock` and `probe.sock`. The longest path,
  `run/c1/probe.sock`, is under #370's limit of 104 bytes for a user name of 20 letters.
- The helper's protocol keeps JSON, so a Swift helper for a macOS computer later speaks the same
  protocol. `start` carries `seed` instead of `provision`. `result` carries libkrun's error name
  (`krun_result_name`) instead of Apple's domain and code. `place_refused` is never sent.
- `kernel/Image` and its hash ship with Silo, not in `<data>`.

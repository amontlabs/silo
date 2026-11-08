# C1: crun containers against Silo's Linux microVM

Phase C1 of [PLAN.md](../../PLAN.md): measurements only. Nothing is built into Silo.

**Question.** Is an opt-in "container computer" on Linux hosts, run with crun under a hardening
baseline, worth building next to the Linux microVM Silo already has? The purpose agreed with the owner:
a dedicated dev machine whose owner accepts less safety, wants native-speed x86 Linux end-to-end
testing, many agents on limited hardware, and protection from accidents, not from attackers. Never the
default.

## Conclusion

**Worth building, as a clearly labelled option that is never the default, for a machine the owner treats
as disposable.** On the fair pairs (a container running a desktop against a microVM that boots one) it
starts about 2x faster and uses about 5x less memory. The decision is the owner's.

Fair pairs (same runner, same CPU, N=3 medians):

- **One desktop, memory.** A container running Xvfb, PulseAudio, Xfce and the Selkies streamer uses
  **315 MiB** (763 MiB with a headed Chrome window). A microVM, which always boots the desktop, uses
  **840 MiB**.
- **Desktop start.** The container's desktop start steps add up to about **2.3 s** (Xvfb 0.21 s,
  PulseAudio 0.21 s, Xfce 0.67 s, Selkies answering 1.16 s, from `raw/desktop.txt`); the microVM's
  `silo start` plus first `silo exec` takes **5.3 s**.
- **Ten desktops.** Ten container desktops use **about 2 GiB** in total (2015 MiB measured). Ten microVMs
  would use **about 10 GiB**: this is **estimated, not measured**, one fork's 1032 MiB times ten. The
  runner could not hold ten (13849 MiB available, 14940 MiB needed), so the script did not run them.
- **Ten desktops, time.** The 28.4 s to "the last one serving" includes 15 s of fixed sleeps in
  [`mode-desktop.sh`](../../containers/boot/mode-desktop.sh) (lines 47-56: 5 s after Selkies serves,
  10 s before reading memory). The last Selkies answered about **12 s** after the start. That is read
  from the run log, not measured directly: in each run the first desktop printed `selkies-http` at
  7.7, 8.3 and 6.8 s after its own start, and the 28.4 s total less the 15 s of sleeps leaves about
  13 s for the last desktop including its start-up and the check.

Bare shell, not comparable on its own: a container shell is ready after 0.09 s and idles at 4 MiB, a
fork after 0.20 s; the microVM boots a whole desktop and takes 5.3 s and 840 MiB (a fork 13.1 s and
1032 MiB; 10.3 s without a desktop, the best F2 gives). Ten container forks at once take 1.1 s; four
microVM forks take 32 s.

Workload steps (apt, unpack, cargo, make) **are not like for like and are not a speed result**: see
section 3. The microVM ran in a fork of `main` built from the demo recipe, the container from the bare
image, so the package sets differ.

What it costs:

- A container shares the host's kernel, so a kernel bug is a host-account bug; the baseline reduces
  that exposure and does not remove it.
- Chrome only runs with `--no-sandbox`.
- A root helper per container for its network (a veth pair cannot be made rootless).
- Ubuntu's AppArmor guard on unprivileged user namespaces
  (`kernel.apparmor_restrict_unprivileged_userns`) has to be relaxed, host-wide.
- A two-way diff of upper layers (a fork's deleted upper-only file leaves no whiteout).
- Also a second isolation boundary to maintain (seccomp profile, Landlock shim, cgroup delegation).
- Secrets, tokens and isolation evals are untested (below).

Suggested next step if the owner agrees: a prototype phase (C2) of one container computer on a Linux
host, driven by `silod`, with the baseline below, its own proxy for secrets, and the isolation evals
run against it.

**What C1 did not probe or run.** Not probed: `clone`/`setns` with `CLONE_NEWUSER` (only `unshare` was
tried from inside), non-TCP traffic. Not run: Silo's secrets proxy, the GitHub token boundary, the guest
client, the isolation evals, a Cua Driver screenshot, a Selkies client session, bare metal.

## Where and how it ran

- **Machine.** GitHub-hosted `ubuntu-24.04` runner, 4 cores, 15.6 GiB, kernel 7.0.0-1017-azure, x86_64.
  **It is a cloud VM**: the microVM runs on nested KVM; the containers run directly on the runner's
  kernel (one level of virtualization below them, none above). A bare-metal Linux host was not
  measured. The runners differ in CPU between runs (EPYC 9V74 in the main run, EPYC 9V45 and 7763 in
  others), so each result names its run.
- **Main comparison (same job, same CPU, AMD EPYC 9V74).** Workflow run
  [42429983339](https://github.com/amontlabs/silo/actions/runs/42429983339), job `microvm`: the Silo
  Dev engine built from the pull request's merge commit `2b6afeb8` (head `732130f4`, main at
  `569f6fea`, so with phase F2), then the microVM measurements, then the engine stopped and the
  container measurements on the same runner. Evidence: [`same-runner/`](same-runner/).
- **Second sample (EPYC 9V45).** The same container script alone in the job `containers` of that run:
  [`second-sample/containers/`](second-sample/containers/). It agrees with the main run; the figures
  below are the main run's unless the text says otherwise.
- **Workflow and scripts.** [`containers-c1.yml`](../../../../.github/workflows/containers-c1.yml)
  (triggered only by changes under `docs/forks-ci/containers/**` and by hand; the microVM job also needs
  the `containers-microvm` label or a manual run. **A pull request carrying the `containers-microvm`
  label re-runs the long microVM job on every push.**) and [`docs/forks-ci/containers/`](../../containers/).
  Nothing in the normal CI changes: the paths are not shared with other workflows, and
  `app/SiloUI/scripts/ci-native-changes.py` already treats `docs/` as skippable (this pull request's own
  Rust jobs run only because it adds a workflow file).
- **Repeats.** N=3 for every row unless it says N=12 or N=30; each cell is the median with the minimum
  and maximum. Times are wall clock (`date +%s.%N` in the container, the monotonic clock of the driver
  for the microVM `silo` commands). PSS is `/proc/PID/smaps_rollup` summed over the processes of the
  container's cgroup (or of the microVM's `msb` process tree, as in
  [F1](../f1-before/linux-kvm-gh/README.md)), 5-20 s after ready.

### Versions

| | |
|---|---|
| Kernel | 7.0.0-1017-azure; cgroup v2; overlayfs; LSMs `lockdown,capability,landlock,yama,apparmor,ima,evm` |
| **Landlock ABI** | **8** on this kernel; at least 4, so no warning. The shim uses the rights of ABI up to 4 (filesystem, TCP bind and connect), and prints a warning, and runs without network rules, below 4 |
| crun | 1.14.1 (`+SYSTEMD +SELINUX +APPARMOR +CAP +SECCOMP +EBPF +WASM:wasmedge +YAJL`), driven directly with a generated `config.json`, `--cgroup-manager=cgroupfs` |
| podman | 4.9.3, only to fetch the image and export its root filesystem |
| Proxy | **Squid 6.14** for the workloads (a mature caching forward proxy, run here as the user, no cache); **tinyproxy 1.11.1** only for the comparison below |
| nftables | 1.0.9 |
| Image | `ghcr.io/amontlabs/silo-guest:ubuntu-24.04-v5-amd64`, image id `sha256:cff964f3…` equal to `imageDigest` in `image-lock.json` (checked); 1158 MiB unpacked. The x86_64 image is published, no substitute |
| Chrome | Google Chrome stable from dl.google.com, installed with apt on top by a container with the relaxed capabilities below (the guest image has no browser; the version was not captured) |
| Engine (microVM) | `silo 0.1.0`, Dev channel, built from the merge commit |

### The baseline that was measured

Generated by [`mkconfig.py`](../../containers/mkconfig.py); proof from inside the container in
[`raw/sec.txt`](same-runner/containers/raw/sec.txt).

- **crun rootless.** The caller is an ordinary user. [`userns.py`](../../containers/userns.py) makes the
  outer user namespace the way rootless podman does (container root is the user, ids 1-65535 are its
  `/etc/subuid` range, mapped by `newuidmap`), mounts the overlay root in it, and runs crun there.
- **No capabilities** (bounding, effective, permitted, inheritable, ambient all `0`), **`no_new_privs`**
  (`NoNewPrivs 1`).
- **seccomp** (`Seccomp 2`), a deny-list over allow-all: `io_uring_setup/enter/register`, `keyctl`,
  `add_key`, `request_key`, `bpf`, `perf_event_open`, `userfaultfd` return EPERM; `clone`, `unshare`
  and `setns` with `CLONE_NEWUSER` return EPERM; `clone3` returns ENOSYS so libc falls back to `clone`.
  Every call was tried from inside and got the expected error; `getpid` as a control worked.
  `unshare -U` fails with "Operation not permitted". This is weaker than Docker's default allow-list
  profile, which was not used.
- **Landlock** applied by [`silo-init.c`](../../containers/silo-init.c) (a 173-line static C program, the
  container's first process, then it execs the command). Read and execute everywhere; write only under
  `/tmp /var /run /home /root /workspace /dev /usr /etc /opt /srv /mnt`; TCP connect only to port 3128
  (plus the ports a test names), TCP bind only to named ports. From inside: `mkdir /boot/x` and
  `touch /media/x` are denied, `/tmp` and `/usr/local` work, `connect(127.0.0.1:9)` gives EACCES
  (Landlock) while `127.0.0.1:3128` gives ECONNREFUSED (allowed by Landlock, nothing listening).
- **Network.** Its own network namespace; a veth pair to the host (`10.200.N.1/30` host,
  `.2` container); **nftables inside the container's namespace**, input and output default-deny, output
  allowed only to the host end's port 3128, everything else `reject` with a counter; on the host a
  separate table drops forwarding to and from the veths and allows only port 3128 in. The proxy is
  Squid on `0.0.0.0:3128`, clients `10.200.0.0/16`, CONNECT to port 443 only. Names are
  resolved by the proxy; a direct lookup from the container fails.
- **Root.** Overlayfs mounted without privilege in the user namespace (`userxattr`): lower layers are
  the Silo boot layer (the shim and test scripts) over the unpacked guest image, upper layer per
  container on ext4. "Read-only" applies to the image layers; `root.readonly` is false so the upper layer
  takes writes, as the microVM's disk does.
- **Masked paths** are the runc/crun defaults (`/proc/kcore`, `/proc/keys`, `/proc/acpi`, `/proc/sched_debug`,
  `/sys/firmware` and others masked; `/proc/sys`, `/proc/bus`, `/proc/fs`, `/proc/irq`,
  `/proc/sysrq-trigger` read-only); `/sys` read-only. Verified: writes to `/proc/sys` and `/sys` fail with
  "Read-only file system", `/proc/kcore` and `/proc/keys` read as 0 bytes.
- **cgroup v2 limits**: memory 4 GiB, cpu 2 cores, pids 4096, verified in the container's cgroup
  (`memory.max 4294967296`, `cpu.max 200000 100000`, `pids.max 4096`). The cgroup tree is delegated to
  the user by a root step (what a systemd `Delegate=yes` slice gives); this is not a systemd user
  manager.
- **No Docker or Podman socket, no host folders** (the mounts are the OCI defaults plus the overlay; the
  boot layer is an overlay lower layer, not a bind mount).
- **Host changes the baseline needs** (printed by `setup.sh`): `kernel.apparmor_restrict_unprivileged_userns`
  set to 0 (it is 1 by default on Ubuntu 24.04, and then no unprivileged program can use its user
  namespace); sub-ids for the user (present); a root step per container for the veth and its nftables.

## Results

### 1. Start, memory and fork

| | Container | microVM (this runner) |
|---|---|---|
| Cold start to a ready shell (bare shell; the microVM boots a desktop) | **0.09 s** (0.08-0.09); the process is running after 0.04 s | **5.31 s** (5.29-5.32): `silo start` 5.09 s + first `silo exec` 0.23 s |
| Creating a new computer first | not needed (the image is shared) | 13.37 s (13.06-13.39) |
| Memory, one idle (bare shell; the microVM includes a desktop) | **4.1 MiB** PSS (4.1-4.2) | **840.8 MiB** (830.9-841.5) |
| Ten started at once | **0.82 s** (0.77-0.84) to the last ready shell; 10.0 MiB in total (10.0-10.1), 1.0 MiB each (N=30) | not measured (see the forks below) |
| Fork, one (new computer from a disk-only copy of the source's disk) | **0.20 s** (0.19-0.20): copy 0.07 s of a 101 MiB upper layer + start | **13.10 s** (13.10-13.12) cold, 13.17 s right after `silo up`; **10.28 s** (9.63-11.09) without a desktop |
| Four forks at once | not run (ten were) | 32.09 s (31.21-32.37) |
| Ten forks at once | **1.10 s** (1.09-1.10): ten copies in parallel 0.24 s + ten starts; 10.0 MiB in total | **not measured**: 13849 MiB available, the script needs 14940 MiB |
| Memory per fork | 1.0 MiB each when ten run | 1031.7 MiB (1018.0-1042.5, N=12) |
| One desktop running | 315.4 MiB (Xvfb, PulseAudio, Xfce, Selkies), 762.7 MiB with a headed Chrome window; about 2.3 s of start steps | included in the 840 MiB and 5.3 s above (the guest image always starts it): the fair pair for this row |
| Ten desktops at once | **28.38 s** (28.02-28.43) to the last one serving, of which 15 s are fixed sleeps (about 12 s to the last Selkies answer, see the conclusion); **2014.7 MiB** in total (2013.3-2024.0), the host used 3.0 GiB | not measured: ten microVMs do not fit the runner; about 10 GiB estimated as 1032 MiB x 10 |

Notes. The fork copy is `cp -a` on ext4 (no reflink), of a 101 MiB upper layer that was in the page
cache; a cold-cache copy and a larger upper layer were not measured. PSS of containers is low because the
pages of the shared image are divided between the containers that map them; a single container alone
shows the full figure (4.1 and 315 MiB). A container fork copies disk only, as F2's microVM forks do:
there is no memory checkpoint for a container. For reference, F1 (before F2, on a Xeon 8370C runner)
measured the microVM cold fork at 11.03 s, `-n 4` at 36.20 s and 1027 MiB per fork
([F1 evidence](../f1-before/linux-kvm-gh/README.md)); `measure_forks.sh` was re-run in this job
([`same-runner/microvm/measure-forks.log`](same-runner/microvm/measure-forks.log)) because the engine
changed with F2 and the CPU differs.

### 2. Does the desktop work under the baseline?

As the image's unprivileged user (uid 1000) with no capabilities, seccomp, Landlock and the network
policy ([`raw/desktop.txt`](same-runner/containers/raw/desktop.txt)):

| | Result |
|---|---|
| Xvfb 1440x900 | works, ready after 0.21 s |
| PulseAudio | works, 0.21 s |
| Xfce (`dbus-run-session startxfce4`) | works, `xfwm4` and `xfce4-panel` up after 0.67 s |
| Selkies 2.0.0 streamer | works: the web page answers HTTP 200 after 1.16 s and the process is alive 5 s later. Landlock needed port 6901 (and 8080) named for bind and connect. **Not tested**: a browser client streaming video, audio, input |
| Cua Driver 0.38.0 | starts as an MCP server (`initialize` 0.18 s), lists 66 tools, and `list_apps` and `list_windows` answer with the real windows ("Desktop", "xfce4-panel"). **Not tested**: a screenshot (no tool name has "screenshot" in it; `get_window_state` was not called), clicks, typing |
| Chrome with its sandbox on | **fails**: exit 134, "setuid sandbox: failed to move to new namespaces", "Zygote process exited prematurely". Chrome's sandbox needs a user namespace, which the seccomp profile blocks |
| Chrome with `--no-sandbox` | works: headless `--dump-dom about:blank` prints `<html><head></head><body></body></html>`, and a headed window appears in Xvfb after 0.22 s |

What `--no-sandbox` means: Chrome stops isolating its renderer, GPU and other helper processes from
each other and from the rest of the container user's files; a compromised web page then runs with
everything the container user can reach (still inside the container's baseline: no capabilities,
seccomp, Landlock, network policy). Allowing user
namespaces in the container (to keep Chrome's sandbox) would give up the baseline's nested-user-namespace
rule, which exists because those namespaces are a frequent kernel-bug entry point.

### 3. A developer workload

The same script ([`boot/workload.sh`](../../containers/boot/workload.sh)) in a fresh container and in a
fresh microVM fork each run, as root, times measured inside: a full `git clone` of expressjs/express,
`apt-get update`, `apt-get install` of build-essential, nodejs and npm (download and unpack/configure
timed apart), `npm install` in the clone, rustup minimal, `cargo install --locked hexyl`, a
`--depth 1` clone of zlib and `./configure && make -j2`. The container has 2 cpus and 4 GiB like the
microVM. **The two sides are not like for like, and these rows are not a speed result.** The microVM
ran in a fork of `main` built from the demo recipe
(`docs/v1/checks/final-proj/.silo/recipes/demo.yaml` pre-installs build-essential, jq, ripgrep, python3,
cron and git; see [`microvm.sh`](../../containers/microvm.sh) lines 47 and 77), the container from the
bare image, so the package sets differ (the container's apt install has more to unpack). The container's
network also goes through the proxy and the microVM's through MicroSandbox's gateway, so the phases
that download differ too.

| Phase (seconds, median, min-max, N=3) | Container | microVM |
|---|---|---|
| git clone express | 1.32 (1.29-1.37) | 1.46 (1.43-1.46) |
| apt update | 4.52 (4.21-6.66) | 1.42 (1.30-1.63) |
| apt install, download | 61.60 (31.97-64.76) | 15.00 (9.06-17.59) |
| apt install, unpack and configure | **18.19** (17.08-21.26) | 22.84 (21.54-23.28) |
| npm install | 12.71 (12.56-12.90) | 10.75 (10.69-10.94) |
| rustup | 8.03 (8.02-8.65) | 9.83 (9.77-10.23) |
| cargo install hexyl | **15.03** (14.52-15.21) | 17.50 (17.28-17.81) |
| git clone zlib | 0.75 (0.67-0.75) | 0.90 (0.78-0.93) |
| configure and make zlib | **2.56** (2.53-2.66) | 2.88 (2.87-3.55) |
| Whole run | 128.20 (95.73-130.10), includes starting the container | 80.98 (77.58-86.22) |

- **Not a speed result.** The steps that are mostly unpacking or compiling were shorter in the container (unpack 20 %, cargo build 14 %, make 11 %), but with different package sets they cannot be quoted as the container being faster (second
  sample, EPYC 9V45: unpack 14.77 s, cargo 12.29 s, make 2.03 s; the microVM there, in the earlier run
  42425963473, took 16.66, 13.75 and 2.13 s, kept in [`second-sample/microvm/`](second-sample/microvm/)).
  The microVM runs on nested KVM here.
- **Downloading through a proxy is slower and varies a lot.** apt makes hundreds of small requests, and
  the proxy adds a hop for each. The first proxy tried, tinyproxy, took **198.87 s** (195.35-206.17) for
  the apt download that Squid did in 69.56 s (64.71-75.64) in the same comparison step (the table's
  61.60 s is the workload runs; in the second sample the pair was 156.95 s against 34.73 s); the
  microVM's 15 s is without a proxy. Squid's own runs spread from 23 to 75 s, and that spread is unexplained. **No control ran the apt
  download through Squid from the host without a container**, so how much of the time is the container
  and how much is Squid is not separated; the 3-5x effect is between tinyproxy and Squid only. One hop
  also costs on a single large file: 39 MB (39,223,207 bytes) took 1.86 s through Squid against 0.92 s
  direct from the host. What
  Silo's real proxy would cost is not measured; the placeholder proxy of boundary 3 is MicroSandbox's,
  and C1 did not run it.
- The upper layer after the workload was 1273 MiB (the build tools and caches).
- apt works as container root **with every capability dropped**: the container root owns the image's
  files (the user-namespace mapping), so `dpkg` needs no `CAP_DAC_OVERRIDE` or `CAP_CHOWN` for the
  packages tried (jq, build-essential, nodejs, npm, Chrome's dependencies), with `-o APT::Sandbox::User=root`
  because it cannot drop to `_apt`. Packages that need a file owned by another user, setuid files or file
  capabilities were not tried.

### 4. Fork of a disk, the manifest diff, the network path

**Fork and manifest.** Container A wrote 200 small files and a 100 MiB file under `/workspace`, a
file under `/root`, appended to `/etc/motd`, created `/etc/c1-marker`, and deleted a file of the image.
Its upper layer: 101 MiB, 214 entries. Container B is a fork: `cp -a` of A's upper layer, then a new
container on it. B saw all of A's changes (`FS_B_SEES files=201 big=104857600 a=from A marker=yes
notices=gone`), then edited one file, added one, deleted two.

- **A against the image** ([`manifest-a.tsv`](same-runner/containers/raw/manifest-a.tsv)): 206 entries,
  205 added, 0 modified, 1 deleted, listed in 0.003 s by [`diff_upper.py`](../../containers/diff_upper.py).
- **B against A, what the fork changed** ([`manifest-b.tsv`](same-runner/containers/raw/manifest-b.tsv)):
  exactly the four changes, `M workspace/proj/f1.txt`, `A workspace/proj/b-new.txt`,
  `D workspace/proj/f2.txt`, `D etc/c1-marker`, in 0.139 s.
- **How the diff works, and what it must handle.** An overlay mounted in a user namespace marks a
  deletion of an image file with a 0/0 character device in the upper layer (a "whiteout"). The first
  version of the script looked for an xattr and listed the whiteout as a modified file. And: when a fork
  deletes a file that only its parent's upper layer holds, **no whiteout is made** (nothing below hides
  it), the file is just missing; so the fork's manifest must compare the two upper trees both ways, not
  just read whiteouts. This is the design constraint Silo's diff would have.

**Network path.** From inside a container ([`raw/net.txt`](same-runner/containers/raw/net.txt)):

| Attempt | Result |
|---|---|
| `curl https://example.com` through the proxy | 200 |
| `curl http://example.com` through the proxy | 200 |
| `curl --noproxy '*' http://1.1.1.1/` (direct to an address) | **refused**: "Failed to connect", nft `reject` |
| `curl --noproxy '*' https://example.com` (direct, by name) | no resolution: there is no resolver reachable |
| host service on another port of the veth address (canary on :8000) | **refused**; the canary's log has no request from the container |
| port 22 of the host's veth address | denied by Landlock (EACCES) |
| a neighbour address in the container subnet range | refused |
| the proxy port with a plain (non-proxy) request | reachable, HTTP 400: this is the one open port, by design |
| a loopback server inside the container | works (port named for bind and connect) |

The container's own reject counter read 7 packets (472 bytes) after these attempts; the host's drop
counters stayed at 0, because nothing got past the container's rules. Squid's access log lists the
container's requests by address and URL.

## What did not work, and why

- **Rootless crun straight on the host cannot give a container an overlay root.** crun mounts nothing
  for `root.path`, and an unprivileged user cannot mount an overlay in the host's mount namespace. The
  fix is the outer user namespace described above (the way rootless podman runs). Two things had to be
  right in it: a user-namespace id range maps only inside one extent of its parent, so the container's
  ids are given as two ranges (0, then 1-65535) matching the outer namespace's two; and raising
  `RLIMIT_NOFILE` above the user's hard limit fails, so the config sets no rlimit.
- **The cgroup move.** crun could not write the container's `cgroup.procs` until the shell that starts it
  was itself inside the delegated cgroup subtree. Delegation here is a root step that creates and
  `chown`s a subtree, not a systemd user slice; a systemd user manager with `Delegate=yes` was not
  tried.
- **A veth pair needs root.** Creating it, moving one end into the container's namespace and writing
  the container's nftables rules are done by a `sudo` step (`net_setup` in
  [`lib.sh`](../../containers/lib.sh)). The container never holds a capability over that namespace.
  A rootless alternative (pasta, slirp4netns) would not be a veth pair and was not measured.
- **Chrome's sandbox** (above).
- **tinyproxy** was the first proxy and made apt's download 3-5 times slower than Squid in the two
  kept comparisons (above); the numbers with it are in the `proxycmp` rows of the summaries.
- **The first CI runs** failed on the harness (a pipe held open by the proxy daemon, the work directory,
  the ranges above, the cgroup); none of their output is used.
- **Not measured, and why.** Bare-metal numbers (no such machine online: the owner's Linux host is
  offline); ten microVMs at once and ten microVM desktops (the runner's memory does not fit them;
  the script says so itself); a screenshot through Cua Driver and a Selkies client session (no client
  in the job); a larger or cold-cache fork copy; reflink forks (ext4 here); CPU contention under ten
  busy containers; secrets, GitHub tokens and the guest client in a container (outside C1); the
  isolation evals (none was run against a container).

## Files

- Scripts: [`docs/forks-ci/containers/`](../../containers/) (`setup.sh`, `run.sh`, `microvm.sh`,
  `lib.sh`, `proxy.sh`, `userns.py`, `mkconfig.py`, `silo-init.c`, `diff_upper.py`, `pss.py`,
  `mapstest.py`, `boot/`).
- Evidence: `same-runner/microvm/` (`summary.txt`, `raw/`, `measure-forks.log`, `f1-rerun/`),
  `same-runner/containers/` (`summary.txt`, `raw/`, `run2.log.gz`, `setup2.log`),
  `second-sample/` (the same, from the other runs). The proxy logs (several MB) and the kernel log are
  in the workflow artifacts (`containers-c1-evidence`, `microvm-c1-evidence`,
  `containers-c1-evidence-same-runner`), kept 30 days.

# What each result means

Written by `./kit doc` from the `# outcome` lines at the top of each test, which are the
only place these meanings are kept. Each test ends on one outcome; `./kit report` prints it
next to the line below, so a run's summary says which ruling it moves.

The entries named are entries of the plan (`plan show <id>`) or the research of
24 November 2026 (`research/2026-11-24-first-build-decisions/`). A result moves nothing by
itself: it is evidence for the owner, or for agents on an entry agents may take.

## 00. Download macOS and install the base computer

How long do the download of Apple's restore image and the install of macOS into a new computer take on this Mac, and does the install work at all?

- Decides: computer-image, first-run-time-macos, which-macos-the-mac-runs (file 10 of the research)
- Tier: `setup`. Step: needed for step 2 of the first build. Time: about an hour (download of about 18 GB, then the install); minutes when already done. Needs: macOS 27 on Apple silicon, 100 GB free, the VM helper (./kit build), no sudo.
- Script: [`tests/00-install-base.sh`](tests/00-install-base.sh)

| Outcome | What it means |
|---|---|
| `installed` | The base computer is installed and was never started; every vm test clones it. The download and install times are the two long waits of first-run-time-macos before the first start; computer-image keeps this installed system. |
| `reused` | A base computer was already there and is reused; nothing was measured this time (delete ~/SiloKit/base/installed.silovm to measure again). |
| `download-failed` | The restore image could not be downloaded; the sentence says why. Rerun: the download resumes where it stopped. |
| `install-failed` | VZMacOSInstaller failed; the error's domain, code and name are in the sentence. computer-image's install step needs a retry or a fix before anything else. |

## 01. Provisioning with the Mac's own login name

Does Apple's guest provisioning accept the Mac's own login name as the computer's user, and if not, at which stage and with which error is it refused?

- Decides: moving-in-on-macos, step 5 of file 10 (how a new macOS computer gets its user and SSH)
- Tier: `vm`. Step: needed for step 2 of the first build. Time: about 30 minutes (two or three first starts). Needs: the base computer of test 00, macOS 27, no sudo; a person only for the optional look at Setup Assistant.
- Script: [`tests/01-provision-user-name.sh`](tests/01-provision-user-name.sh)

| Outcome | What it means |
|---|---|
| `own-name-works` | The Mac's own login name was provisioned and SSH answered. moving-in-on-macos holds as written: main's user is your user name, set by provisioning, with no setup user. |
| `own-name-refused` | Provisioning refused the Mac's own login name (the stage and error are in the sentence) and another name worked. moving-in-on-macos needs file 10's workaround: provision silo-setup, then make your user over SSH (step 7). Report FB23241619 as still open. |
| `own-name-silent` | No error, but SSH never answered for the Mac's own login name, while another name worked: the failure shows only as a timeout (and Setup Assistant, if a person looked). Same workaround as own-name-refused, and Silo must treat "SSH does not answer" as a failure. |
| `both-fail` | Neither name gave SSH: provisioning itself does not work on this Mac or this build. Look at the attached run logs before ruling anything. |
| `inconclusive` | Something else happened; the sentence and the attached logs say what. |

## 02. Claude Code and Codex signed in over SSH: reconnect, restart, checkpoint, fork

Signed in over SSH inside a macOS computer, where does each program keep its login (a file or the keychain), and does the login still work after a reconnect, in a tmux or screen session that outlives its SSH connection, after a restart of the computer, in a copy restored from a checkpoint and in a fork with a new identity?

- Decides: fork-and-sign-in (where each login lands), runs-outlive-the-terminal (a login used from a session left running after its terminal closed), whether the first build's "ssh main, start Claude Code, go to bed" holds, agent-programs-logins-kept-out-of-copies
- Tier: `guided`. Step: needed for step 2 of the first build. Time: about 45 minutes, with you at the keyboard for the sign-ins (about 5 minutes). Needs: the installed computer of test 00, macOS 27 on the Mac, network, a Claude subscription or Console account and a ChatGPT account (either one is enough).
- Script: [`tests/02-agent-login-over-ssh.sh`](tests/02-agent-login-over-ssh.sh)

| Outcome | What it means |
|---|---|
| `lasts-in-files` | Both logins landed in files (~/.claude/.credentials.json, ~/.codex/auth.json) and worked after reconnecting, after a restart, in the checkpoint copy and in the fork: "start Claude Code, go to bed" holds over SSH; fork-and-sign-in can keep a login out of a fork by leaving those files out, as it says. |
| `lasts-in-keychain` | The logins kept working, but at least one landed in the login keychain: a fork copies it and Silo cannot leave one keychain item out (research 06, cross-cutting 1); fork-and-sign-in must say, for macOS computers, that this login is copied. |
| `lost-somewhere` | A login that worked right after signing in failed later; the facts name the first stage that failed for each program (reconnect, a session left running in tmux or screen after its SSH connection closed, restart, checkpoint, fork). Before the first build, the story "go to bed" changes, or Silo signs agents in another way (claude setup-token). |
| `inconclusive` | No login worked right after signing in, or the computer did not come back; read the facts and the attached checks. |

## 03. APFS clone time against fragmentation, with a writer

Does cloning a disk image stall a program writing to it, and does the stall grow with the pieces checkpoints leave the image in, raw and ASIF?

- Decides: checkpoint-per-system, speed-budgets (the checkpoint's target), research 07 "What to do if APFS behaves like XFS"
- Tier: `quick`. Step: needed for step 4 of the first build. Time: about 25 minutes (stops early past its time limit). Needs: 30 GB free, no VM, no sudo (the ASIF part asks for sudo only if its device is not yours).
- Script: [`tests/03-apfs-clone-stall.sh`](tests/03-apfs-clone-stall.sh)

| Outcome | What it means |
|---|---|
| `no-stall` | At 1000 checkpoints a write waited at most 0.1 s during a clone: checkpoints need none of research 07's mitigations; the checkpoint's target can be written as the clone time. |
| `clone-slow-writes-free` | The clone itself grows past 1 s with the pieces but writes do not wait for it (0.1 s or less): a checkpoint delays the model's reply (checkpoint-at-each-turn) but not main; the target names the clone time. |
| `stall-small` | The longest write wait during a clone stays between 0.1 and 1 s at 1000 checkpoints: write the checkpoint's target as that wait at that fragmentation, and count pieces at each checkpoint (research 07 step 1, "measure and show"). |
| `stall-grows` | A write waits more than 1 s during a clone at 1000 checkpoints, growing with the pieces as on XFS (11 to 23 s at about 500,000 pieces): checkpoint-per-system needs research 07's mitigations, making the disk whole when main stops first, a layer per boot next. |
| `inconclusive` | The runs did not get far enough or disagree; look at 03-steps.csv and the stall lines in the log. |

## 04. Is a clone of a running computer's disk a moment in time, per disk mode

When the Mac clones the disk of a running macOS computer, does the clone keep every write the guest flushed before it, with no write missing before a later one, in each of the 9 caching and sync modes, with and without the guest flushing?

- Decides: checkpoint-per-system (the mode main's disk is attached in; whether a checkpoint needs a pause), whole-computer-grain
- Tier: `vm`. Step: needed for step 2 of the first build. Time: about 60 minutes (19 starts of a computer). Needs: the base computer of test 00, 40 GB free, no sudo on the Mac.
- Script: [`tests/04-clone-moment-in-time.sh`](tests/04-clone-moment-in-time.sh)

| Outcome | What it means |
|---|---|
| `all-hold` | In all 9 modes the clone held every flushed write, no hole, and booted: a checkpoint needs no pause; main's disk is attached in whichever mode is fastest (the default, automatic and full, unless test 15 says otherwise). |
| `some-lose-pause-helps` | Some modes lost flushed writes or left holes, and pausing the computer for the clone fixed them: attach main's disk in a mode that held (named in the sentence), or pause for each checkpoint in the others. |
| `some-lose` | Some modes lost flushed writes or left holes, even with a pause: attach main's disk only in a mode that held (named in the sentence). |
| `none-hold` | No mode kept every flushed write: a checkpoint of a running computer is not a moment in time; checkpoint-per-system needs the computer stopped, or another way (research 07). |
| `inconclusive` | A computer did not start or a writer did not run; look at 04-modes.csv and the logs attached. |

## 05. Which blocks two clones differ in, from the Mac's block map

Does F_LOG2PHYS_EXT on two APFS clones of a disk image tell exactly which blocks differ, matching a byte comparison, and how fast?

- Decides: file-changes-from-outside (catching writes past APFS), research 11 "Where the trust comes from", the "how much changed" of a gap
- Tier: `quick`. Step: needed for step 5 of the first build. Time: about 5 minutes. Needs: 12 GB free, no VM, no sudo.
- Script: [`tests/05-log2phys-diff.sh`](tests/05-log2phys-diff.sh)

| Outcome | What it means |
|---|---|
| `map-tells` | Every block whose bytes differ has a different device place, and few equal blocks are flagged: the engine can list the blocks written between two checkpoints from the Mac's side, and a write past APFS inside main shows (research 11's design point 4). |
| `map-overstates` | No differing block is missed, but many equal blocks are flagged: usable as a bound (what to read), not as a count of what changed. |
| `map-misses` | Some blocks differ in bytes at the same device place: the map cannot be trusted for this; the reader's trust rests on APFS rules alone and the record says writes past APFS are not seen. |
| `no-map` | F_LOG2PHYS_EXT gives no map of a clone here; the same as map-misses for the design. |

## 06. Cost of a forced write, and overwrites after a clone

What do fsync, F_BARRIERFSYNC and F_FULLFSYNC cost, alone, grouped and while another program writes, and does an overwrite take new space once a clone shares the file?

- Decides: engine-saved-state, events-until-the-recorder, low-disk, record-speed-cost
- Tier: `quick`. Step: needed for step 1 of the first build. Time: about 6 minutes. Needs: 3 GB free, the Command Line Tools, no VM, no sudo; run on mains power with other apps closed.
- Script: [`tests/06-fsync-cost.sh`](tests/06-fsync-cost.sh)

| Outcome | What it means |
|---|---|
| `full-fast-reserve-needed` | F_FULLFSYNC's p99 while another program writes is 50 ms or less, and an overwrite after a clone takes new space: the engine flushes each write group with F_FULLFSYNC (engine-saved-state as proposed in research 02), and low-disk keeps a reserve, since preallocating does not protect once a checkpoint or local snapshot shares the file. |
| `full-slow-reserve-needed` | F_FULLFSYNC's p99 under a writer passes 50 ms, and an overwrite after a clone takes new space: the engine groups its writes (one flush per group, ticks never inputs), core-never-waits on it, and low-disk keeps a reserve. |
| `full-fast-prealloc-protects` | F_FULLFSYNC is 50 ms or less under a writer, and an overwrite after a clone took no new space (not expected on APFS): preallocating the engine's files is enough, no reserve. |
| `full-slow-prealloc-protects` | F_FULLFSYNC passes 50 ms under a writer, and an overwrite after a clone took no new space: group the engine's writes; preallocating is enough. |
| `inconclusive` | A measurement failed or the space readings moved too much; look at 06-fsync.csv and the space lines. |

## 07. gvproxy with a macOS computer, against Apple's NAT

Does a macOS computer work on gvproxy (DHCP, MTU, ssh through a Unix socket forward, bursts of connections, git clone and npm install, measured against Apple's NAT), and what of the Mac can it reach through each?

- Decides: network-stack, computers-main-on-a-mac, who-can-call-the-engine
- **Blocks: option A of the first build (#373): main is on Silo's network stack from its first start, so this runs before main does.**
- Tier: `vm`. Step: needed for step 2 of the first build. Time: about an hour. Needs: the base computer of test 00, a 27 MB download of gvproxy, about 1 GB downloaded inside the computer (Node, git, npm packages), no sudo; you only for the optional sleep step.
- Script: [`tests/07-gvproxy-macos-guest.sh`](tests/07-gvproxy-macos-guest.sh)

| Outcome | What it means |
|---|---|
| `gvproxy-now` | gvproxy works for a macOS computer, within 1.5 times of Apple's NAT, and the hardened configuration keeps the Mac's 127.0.0.1 and gvproxy's API out of its reach. network-stack: take it with gvproxy as the first driver, with research 04's edits (NAT entry removed, API token, ssh through a Unix socket). Read the reach facts: the Mac's LAN address is still reachable through any stack that dials out for the computer, so device-out-of-reach needs a rule in the forwarder. |
| `gvproxy-slow` | gvproxy works but npm install or git clone takes more than 1.5 times as long as on Apple's NAT. network-stack: gvproxy stays the driver only with the MTU raised (silovm needs an MTU option) or linked into the helper; compare the numbers before taking it. |
| `gvproxy-stalls` | bursts of new connections stall (over 5 s) or fail on gvproxy and not on Apple's NAT: the TIME_WAIT bug research 04 names. network-stack: gvproxy needs the gVisor fix (PR 17844) or a workaround before agents run all night on it; the record should carry the stack's own stalls as facts. |
| `gvproxy-fails` | no DHCP lease or no ssh through the forward for a macOS computer. network-stack's fallback: the first build runs main on Apple's NAT and shows that main's network has no boundary. |
| `hardening-fails` | the hardened configuration still lets the computer reach the Mac's 127.0.0.1 or change gvproxy's port forwards. Silo must patch gvproxy (or link it with its own forwarder), not only configure it, before network-stack can say the computer reaches neither the Mac's loopback nor the Mac. |
| `inconclusive` | something needed for the comparison did not run (no NAT phase, no internet inside); the notes say which, and the numbers that did run are in the results. |

## 08. Claude desktop and the Codex app through an included SSH entry with fd passing

Does the Codex app list a host that only an Included file defines, and do Claude desktop and the Codex app connect through a ProxyCommand that hands ssh its socket (ProxyUseFdpass), as Silo's entry would?

- Decides: reaching-a-computer, computer-reached-over-ssh, agent-programs-reach-every-computer
- Tier: `guided`. Step: needed for step 2 of the first build. Time: about 20 minutes. Needs: you at the keyboard, Claude desktop and/or the Codex app (or the ChatGPT app with Codex) installed and signed in, the base computer of test 00, ./kit build (c/sshproxy.c); one line added to ~/.ssh/config for the length of the test.
- Script: [`tests/08-codex-claude-ssh-config.sh`](tests/08-codex-claude-ssh-config.sh)

| Outcome | What it means |
|---|---|
| `include-and-fdpass-work` | the Codex app lists the included host, and each app tried connects through the fd-passing ProxyCommand. reaching-a-computer: take "one line that includes Silo's file"; the SSH transport: a Unix socket per computer and a ProxyCommand with ProxyUseFdpass, as research 06 proposes. |
| `codex-ignores-include` | the Codex app does not list a host defined only in the included file. reaching-a-computer: Silo must write each computer's Host lines into ~/.ssh/config itself (or the person pastes them), or the Codex app never lists a computer; the fd-passing results are in the sentence. |
| `fdpass-refused` | an app connects through the relaying ProxyCommand but not through the fd-passing one. The SSH transport: Silo's ProxyCommand relays bytes for the length of each session instead of handing ssh the socket (it stays running; Silo still sees each connection). |
| `proxycommand-refused` | an app connects through neither ProxyCommand. The SSH transport: Silo needs a localhost port shown as a channel, for that app at least. |
| `inconclusive` | no app was tried, or the answers were skipped; or the automatic checks failed (the facts say which). |

## 09. Start a computer from launchd: user agent, system daemon, nobody logged in

Can a program started by launchd, as a user agent and as a system daemon (with you logged in, then with nobody logged in), start a macOS computer that answers SSH?

- Decides: where the engine runs; engine-matches-wanted (does main wait for your login after the Mac restarts)
- Tier: `guided`. Step: needed for step 2 of the first build. Time: about 30 minutes, plus 10 minutes logged out if you agree. Needs: the installed computer of test 00, macOS 27 on the Mac, --allow-sudo for the daemon half (it loads one job in launchd's system domain and removes it).
- Script: [`tests/09-engine-under-launchd.sh`](tests/09-engine-under-launchd.sh)

| Outcome | What it means |
|---|---|
| `agent-and-daemon` | A user agent and a system daemon both started a computer that answered SSH while you were logged in. The engine may be a launchd job; whether it may run with nobody logged in is the logged-out half (run the test again after it, or it was not run). |
| `daemon-refused` | The user agent's computer started, the system daemon's did not (the facts hold the error): Apple's "Virtualization is not daemon safe" holds; the engine runs in your login session (a user agent or the app), and after a restart main waits for your login. |
| `agent-refused` | Even the user agent could not start a computer: the engine must be a program started from your session (an app or a login item), not a launchd job; read the facts. |
| `logged-out-works` | With nobody logged in to the Mac, the daemon started a computer that answered SSH: main could come back after a restart before anyone logs in (FileVault aside). engine-matches-wanted need not wait for your login on a Mac without FileVault. |
| `logged-out-refused` | With nobody logged in, the daemon's computer did not start or did not answer SSH (the facts say which): main comes back only once you log in; engine-matches-wanted should say so. |
| `waiting-for-logout` | The logged-out half is armed. Log out, wait ten minutes at the login window, log back in and run "./kit run 09 --allow-sudo" again: that run reads the result. |
| `sudo-not-allowed` | Only the user agent half ran (no --allow-sudo); the facts hold its result. |
| `inconclusive` | Something else went wrong; read the facts and the attached logs. |

## 10. A first start cut off after 20 seconds

When a computer's first, provisioned start is forced off 20 seconds in, does starting it again with the same options still provision it?

- Decides: computer-image, step 4 of file 10 (is the never-started copy needed for retries)
- Tier: `vm`. Step: needed for step 2 of the first build. Time: about 20 minutes. Needs: the base computer of test 00, macOS 27, no sudo.
- Script: [`tests/10-cut-off-first-start.sh`](tests/10-cut-off-first-start.sh)

| Outcome | What it means |
|---|---|
| `provisioned-after-cut` | The second start provisioned the computer and SSH answered. A cut-off first start can be retried on the same disk; the never-started copy of computer-image is needed only to make later computers. |
| `not-provisioned` | The second start ran but SSH never answered: provisioning is spent by a cut-off first start. A retry must start from a fresh clone of the never-started copy (step 4), so computer-image must keep it. |
| `refused-second-start` | The framework refused the provisioning options on the second start (the stage and error are in the sentence). Same consequence as not-provisioned, and Silo must expect that error on a retry. |
| `inconclusive` | The cut did not happen as planned (the computer stopped by itself, or never started); the sentence and logs say what. |

## 11. Documents, Desktop and Downloads over SSH

In a provisioned computer reached over SSH, can a command list and write ~/Documents, ~/Desktop and ~/Downloads, or does macOS's privacy protection (TCC) refuse it?

- Decides: computer-reached-over-ssh, unmodified-agent-programs (where agents' work can live over SSH)
- Tier: `vm`. Step: needed for step 2 of the first build. Time: about 10 minutes. Needs: the installed computer of test 00, macOS 27 on the Mac.
- Script: [`tests/11-documents-over-ssh.sh`](tests/11-documents-over-ssh.sh)

| Outcome | What it means |
|---|---|
| `open` | Every folder can be listed and written over SSH, as the user and as root: agents can work anywhere in the home folder; the first run needs to say nothing about these folders. |
| `refused` | The protected folders are refused over SSH ("Operation not permitted") while the home folder itself works: agents reached over SSH cannot work in Documents, Desktop or Downloads unless "Allow full disk access for remote users" is turned on inside the computer. Silo either turns it on during setup (it is a switch under Remote Login, see test 22) or keeps work elsewhere (~/work) and the first run says so. |
| `mixed` | Some folders or some actions are refused and others not; the facts name which. Read them before choosing where work lives. |
| `inconclusive` | Even the home folder could not be written, or SSH failed; read the attached log. |

## 12. Twenty stops of a busy macOS computer

Asked to stop while busy, how often does a macOS 27 guest shut down by itself within 30 seconds, and does a stop ever leave one of Apple's two places for macOS computers taken?

- Decides: what-stop-ends, neutral-core (a refused start taken as a fact)
- Tier: `long`. Step: needed for step 2 of the first build. Time: about 1.5 hours. Needs: the installed computer of test 00, macOS 27 on the Mac, no other macOS VM running (UTM, Tart, Parallels).
- Script: [`tests/12-twenty-stops.sh`](tests/12-twenty-stops.sh)

| Outcome | What it means |
|---|---|
| `clean-stops` | At least 18 of 20 busy guests shut down within 30 s of request-stop and no place leaked: what-stop-ends' "ask, wait 30 s, then end it" holds on macOS as written; the 30 s is rarely reached (the facts give the slowest stop). |
| `slow-stops` | More than 2 of 20 needed the forced end after 30 s, and no place leaked: a stop of main usually costs the full 30 s, so a restore waits that long; what-stop-ends should say so, or Silo shuts the guest down over SSH first. |
| `place-leaks` | After a stop, starting two computers failed at least once with the limit error: the leak reported on macOS 26.5 is still there on this Mac. neutral-core must take a refused start as a fact and say "restart the Mac"; the facts say after which stop it began. |
| `inconclusive` | Fewer than 10 rounds completed (a start failed for another reason, or another VM app held a place); read the attached rounds log. |

## 13. Ten fresh computers, and one with a new identity

How long does a fresh clone of the installed computer take from its first start to SSH, how often does it fail, and does a clone with a new machine identifier and auxiliary storage start and provision?

- Decides: computer-image, first-run-time-macos, step 6 of file 10 (the time limit before a retry)
- Tier: `long`. Step: needed for step 2 of the first build. Time: about an hour (eleven first starts in sequence). Needs: the base computer of test 00, macOS 27, no sudo.
- Script: [`tests/13-ten-fresh-computers.sh`](tests/13-ten-fresh-computers.sh)

| Outcome | What it means |
|---|---|
| `reliable-identity-ok` | All ten provisioned, and the clone with a new identity did too. The sentence gives the retry time limit (1.5 times the slowest, rounded up to 30 s) for step 6 and the time for first-run-time-macos; computer-image can make each later computer from the installed system with its own identity. |
| `reliable-identity-fails` | All ten provisioned, but the clone with a new machine identifier and new auxiliary storage did not start or provision. computer-image must keep the installed auxiliary storage (made by VZMacOSInstaller) for each later computer, or install again. |
| `unreliable` | At least one of the ten failed (count, reasons and identity result in the sentence). Step 6's retry from the never-started copy is needed, with the limit given. |
| `all-failed` | None of the ten gave SSH: look at test 01 and the logs first. |

## 14. What provisioning leaves in a new computer

After Apple's provisioning, is FileVault off, does the user hold a secure token, is SSH served, is the login keychain unlocked in an SSH session, and what do auto-login, sleep, the screen lock and softwareupdate look like?

- Decides: the checks before main is called ready (research 10, step 8), first-run-time-macos, file-changes-from-outside (FileVault off)
- Tier: `vm`. Step: needed for step 2 of the first build. Time: about 15 minutes. Needs: the installed computer of test 00, macOS 27 on the Mac, network (softwareupdate asks Apple).
- Script: [`tests/14-what-provisioning-leaves.sh`](tests/14-what-provisioning-leaves.sh)

| Outcome | What it means |
|---|---|
| `ready-checks-hold` | FileVault is off, the user has a secure token and owns the volume, sshd runs, and the login keychain is unlocked and writable over SSH: the checks of step 8 can be written as they are, and an agent signed in over SSH may write the keychain (see test 02). |
| `keychain-locked-over-ssh` | Everything else holds, but the login keychain is locked (or not writable) in an SSH session: step 8's "keychain unlocked over SSH" check fails by design; Silo either unlocks it (security unlock-keychain with the password it keeps) or expects logins in files, as Claude Code's documentation says it falls back to. |
| `setup-differs` | FileVault is on, the user has no secure token, or sshd is not served as expected: the facts say which; step 5 to 8 of research 10 change before the first build. |
| `inconclusive` | The checks could not run; read the attached log. |

## 15. A real macOS computer under agent-like work, a checkpoint per step

How fast does a real macOS computer's disk image fragment under agent-like work with a checkpoint per step, how long does a write inside wait during a checkpoint, and does a checkpoint from mid-run boot clean with its repositories intact?

- Decides: checkpoint-per-system, speed-budgets (the checkpoint's target as the wait inside), restore-to-a-checkpoint, what-a-restore-leaves, research 07 "What to do if APFS behaves like XFS"
- Tier: `long`. Step: needed for step 4 of the first build. Time: 2 to 3 hours (KIT_15_STEPS shortens it). Needs: the base computer of test 00, 120 GB free, a network for the guest (GitHub, npm, nodejs.org), no sudo on the Mac.
- Script: [`tests/15-real-guest-under-work.sh`](tests/15-real-guest-under-work.sh)

| Outcome | What it means |
|---|---|
| `holds` | The restored checkpoint booted clean, git and npm worked in it, and no flushed write inside waited more than 0.1 s during a checkpoint: checkpoints per step are fine as they are; the target can be written at the pieces reached. |
| `waits-small` | Restore was clean; the longest write wait inside during a checkpoint was between 0.1 and 1 s: write the checkpoint's target as that wait at the pieces reached, and count pieces (research 07 step 1). |
| `waits-grow` | Restore was clean but a write inside waited more than 1 s during a checkpoint, growing with the pieces: research 07's mitigations are needed (make the disk whole when main stops, then a layer per boot). |
| `restore-broken` | The mid-run checkpoint did not boot, its volume did not verify, git showed changed tracked files or npm ls failed in it: restore-to-a-checkpoint cannot rest on a clone of a running disk as taken here; look at the facts, then test 04's modes. |
| `inconclusive` | The work or the computer failed before enough steps; look at 15-steps.csv. |

## 16. An idle computer for a night

Left idle for a night, what does a provisioned macOS computer cost the Mac in processor, memory and disk, and which paths does macOS write inside it by itself?

- Decides: mac-kept-awake (whether the computer's process holds the Mac awake by itself, read before the test's own assertion), own-cost-targets and silo-is-light (the idle reference every cost target is read against), the default filter of silo changes
- Tier: `long`. Step: needed for step 3 of the first build. Time: one night (8 hours by default; KIT_IDLE_HOURS sets it). Needs: the installed computer of test 00, macOS 27 on the Mac, the Mac on power and left awake (the test holds a caffeinate assertion while it runs).
- Script: [`tests/16-idle-night.sh`](tests/16-idle-night.sh)

| Outcome | What it means |
|---|---|
| `quiet` | After the first half hour the computer is quiet: the Virtualization process uses under 5% of one core (median of the quiet windows) and macOS changes fewer than 2,000 files per half hour. The numbers are the idle reference of own-cost-targets; ranked.txt is the first default filter of silo changes (folders changed in most windows are hidden by default, with a flag to show them). |
| `busy` | The idle computer keeps working: over 5% of a core, or over 2,000 changed files per half hour after the first half hour. The ranked list says who (Spotlight, software update, logs); Silo turns those off in setup, or the cost targets are read against this higher floor. |
| `inconclusive` | Fewer than three samples or no window of changed files; read samples.tsv. |

## 17. How often a clone of a busy computer's disk has a broken newest APFS checkpoint

Of 100 clones of a macOS computer's disk taken while programs inside write, how many have a newest APFS checkpoint that does not check, and does an older one always open?

- Decides: research 11 "What the reader has to handle" (the fallback), file-changes-from-outside
- Tier: `long`. Step: needed for step 4 of the first build. Time: about 45 minutes. Needs: the base computer of test 00 (raw disk), 40 GB free, no sudo on the Mac.
- Script: [`tests/17-checkpoint-broken-rate.sh`](tests/17-checkpoint-broken-rate.sh)

| Outcome | What it means |
|---|---|
| `never-broken` | No clone had a broken newest checkpoint: the reader's fallback is a rare path; it stays, tested on dissect.apfs's corrupt image. |
| `sometimes-broken` | Some clones had a broken newest checkpoint and an older one opened every time: the fallback is a normal path, the record says which checkpoint it read and that a newer one was skipped. |
| `unreadable` | Some clone had no checkpoint that opens in some container: the reader would give a gap for that checkpoint; Silo needs a pause (test 04) or a retry of the clone before reading. |
| `inconclusive` | The computer or the probe failed; look at 17-checks.jsonl. |

## 18. Leaf-address diff of two clones of a computer with many files (probe only)

On clones of a real macOS computer's disk with many files, does the leaf-address diff give the same changes as a full walk, and do the blocks it reads follow the changes rather than the tree?

- Decides: file-changes-from-outside ("the time follows the size of the tree"), research 11's estimate of a comparison's cost
- Tier: `long`. Step: needed for step 5 of the first build. Time: about 50 minutes. Needs: the base computer of test 00 (raw disk), 40 GB free, no sudo on the Mac.
- Script: [`tests/18-leaf-diff-scale.sh`](tests/18-leaf-diff-scale.sh)

| Outcome | What it means |
|---|---|
| `follows-changes` | The leaf diff matched the full walk every time and read a small part of what the walk read for small changes: research 11's design holds on a real disk; the Python probe's times bound nothing for Silo's reader, which does not exist yet. |
| `always-wide` | The leaf diff matched but read nearly as much as the full walk even for 10 changed files (macOS changes leaves across the tree by itself): the cost follows the tree after all; keep research 11's estimate out of targets until Silo's reader is measured. |
| `wrong` | Some leaf diff differed from the full walk on the same pair: the leaf-address method misses changes on a real disk; research 11's design must change before the reader is built. |
| `no-read` | The probe could not read the clones' data volume (a gap): what it failed on is in the sentence; Silo's reader has to handle it. |
| `inconclusive` | The computer or the probe failed; look at 18-diffs.jsonl. |

## 19. What the APFS reader meets inside a real computer: raw writes, snapshots, a new volume, FileVault

Can root inside a macOS computer write its raw disk with SIP on, can FileVault be turned on inside, and what does the reader see on clones after 100 local snapshots, an added volume and FileVault?

- Decides: research 11 "Where the trust comes from" and "What the reader has to handle", file-changes-from-outside, macos-ships-facts-first
- Tier: `guided`. Step: needed for step 5 of the first build. Time: about 60 minutes; you may be asked to turn on FileVault in the computer's window. Needs: the base computer of test 00 (raw disk), 40 GB free, no sudo on the Mac (sudo runs inside the computer only).
- Script: [`tests/19-reader-gaps.sh`](tests/19-reader-gaps.sh)

| Outcome | What it means |
|---|---|
| `raw-writes-allowed` | Root inside wrote its own raw disk with SIP on: a write past APFS is possible from main, so the reader's trust must come from the Mac's block map (test 05), never from APFS's rules alone. |
| `reader-fails` | The reader failed on one of the cases (named in the sentence): Silo's reader must handle it; a test disk of that case goes in its tests. |
| `all-seen` | Raw writes were refused with SIP on, and the reader listed the 100 snapshots and the new volume and reported FileVault's volume as a gap (or FileVault could not be turned on, as said): research 11's list of cases holds. |
| `inconclusive` | A step did not run; look at the facts and 19-reader.jsonl. (the protective MBR and GPT header, which nothing writes while it runs) and on the APFS physical store of the boot volume (its block 0, the superblock copy APFS rarely writes). Nothing changes; only whether the write is allowed. |

## 20. The engine's socket: who connected, and what exec changes

What does a Unix socket server on the Mac learn about the process that connected (user, pid, audit token, the parent chain up to an app), does the audit token's pid version change when that process runs exec, and is a process of another user kept out?

- Decides: who-can-call-the-engine, reaching-a-computer
- Tier: `quick`. Step: needed for step 1 of the first build. Time: about 2 minutes. Needs: ./kit build (c/peercred.c), no VM, no download; sudo only with --allow-sudo, for the other-user half (otherwise skipped).
- Script: [`tests/20-engine-socket-peer.sh`](tests/20-engine-socket-peer.sh)

| Outcome | What it means |
|---|---|
| `identity-holds` | the caller's user and pid are right in every look, the audit token's pid version changes on exec (so a token kept from accept tells an exec'd caller apart), and another user is kept out. who-can-call-the-engine: same user by the socket's folder and getpeereid is sound; "opened by" can come from LOCAL_PEERPID's parent chain, which the sentence says reaches the app or not (a double fork loses it). |
| `pidversion-stays` | the audit token's pid version stays the same across exec: a check of the caller's program over a Unix socket can be fooled by exec. who-can-call-the-engine: never check a code signature over the socket; use XPC with a peer requirement if that check is ever wanted. |
| `other-user-not-refused` | a process of another user connected and got an answer. who-can-call-the-engine's "refused before any handler runs" does not hold as built here: look at the facts before relying on the socket's folder or getpeereid. |
| `inconclusive` | a look failed (the facts say which), or there is no audit token on this system (the Linux analogue). |

## 21. Do Claude Code and Codex trust the computer's keychain authorities

Inside a macOS computer, does each agent program accept a TLS certificate from an authority trusted in the System keychain, and does it accept one given only in its own setting (NODE_EXTRA_CA_CERTS, CODEX_CA_CERTIFICATE, SSL_CERT_FILE)?

- Decides: reading-each-agent-program on macOS (how model traffic is read in a macOS computer, second build)
- **Blocks: option A of the first build (#373): the first run puts Silo's authority in main, and this says where (keychain, or each program's own setting).**
- Tier: `vm`. Step: needed for step 2 of the first build. Time: about 25 minutes; a person for one approval if macOS asks for it. Needs: the installed computer of test 00, macOS 27 on the Mac, network (downloads Claude Code and Codex inside the computer, about 300 MB).
- Script: [`tests/21-agent-trust-store.sh`](tests/21-agent-trust-store.sh)

| Outcome | What it means |
|---|---|
| `both-trust-keychain` | Claude Code and Codex both accepted the test authority from the System keychain, and refused it when it was nowhere: one trusted authority in the computer's keychain is enough to read both programs' traffic; no per-program setting is needed. |
| `claude-only-keychain` | Claude Code trusts the keychain, Codex does not: Codex needs its own setting (the facts say whether CODEX_CA_CERTIFICATE or SSL_CERT_FILE worked), which Silo must set in the environment of every Codex it starts, or Codex's traffic is not readable. |
| `codex-only-keychain` | Codex trusts the keychain, Claude Code does not: Claude Code needs NODE_EXTRA_CA_CERTS (or CLAUDE_CODE_CERT_STORE) in its environment. |
| `neither-keychain` | Neither program accepted the keychain's authority: each needs its own setting; the facts say which settings worked. |
| `keychain-needs-a-person` | macOS would not trust the authority in the computer's keychain without someone approving it on the computer's screen, and nobody did: Silo cannot set up the keychain way by itself (only a person, a profile from device management, or SIP off can); the programs' own settings (facts) are then the only way, set in each program's environment. |
| `inconclusive` | The control (authority nowhere) was not refused, or no program connected to the test server (a proxy, IPv6, or a DNS cache may have sent it elsewhere): read the facts and the attached server logs. |

## 22. Endpoint Security events inside a computer, from an SSH session

Inside a macOS computer, does eslogger (Endpoint Security) run as root from an SSH session and see an agent-like command's process starts and file writes, and if not, which grant makes it work?

- Decides: denied-tool-call-mechanism and processes-in-the-record for a macOS computer (second build): can they use Endpoint Security inside it
- Tier: `guided`. Step: after the first build. Time: about 20 minutes, with you at the computer's window for one switch (about 2 minutes). Needs: the installed computer of test 00, macOS 27 on the Mac.
- Script: [`tests/22-endpoint-security-in-guest.sh`](tests/22-endpoint-security-in-guest.sh)

| Outcome | What it means |
|---|---|
| `es-over-ssh-works` | eslogger ran as root over SSH with no grant and saw the command's exec and write events: a recorder inside a macOS computer can use Endpoint Security, started over SSH. |
| `es-needs-remote-fda` | eslogger was refused (not permitted) until "Allow full disk access for remote users" was turned on under Remote Login, then saw the events: Silo turns that switch on in setup (a person's click, or a profile; it cannot be scripted from inside without a person), or its recorder runs as a launchd daemon with its own Full Disk Access. |
| `es-refused` | eslogger was refused even with the switch on: Endpoint Security inside a computer needs a signed, entitled program with its own grant (a system extension); the facts hold the errors. |
| `inconclusive` | eslogger ran but saw none of the command's events, or the switch was not turned on; read the attached logs. |

## 23. Xcode copied in from the Mac, and the iOS runtime downloaded in the computer

Does Xcode copied from this Mac into a computer pass Apple's signature check, accept its license and finish its first launch over SSH with nobody at the screen, and does the iOS simulator runtime download there with no Apple Account?

- Decides: xcode-in-a-computer, first-run-time-macos, computer-image (whether the kept image can hold Xcode)
- Tier: `vm`. Step: after the first build. Time: about an hour, most of it copying Xcode and downloading the runtime. Needs: the installed computer of test 00 (80 GB disk), macOS 27 on the Mac, Xcode installed on this Mac (read, never changed), about 40 GB free, network (the runtime comes from Apple's CDN, about 9 GB).
- Script: [`tests/23-xcode-into-a-computer.sh`](tests/23-xcode-into-a-computer.sh)

| Outcome | What it means |
|---|---|
| `ready` | Xcode passed the signature check, its license and first launch were done over SSH, and the iOS runtime downloaded and shows as available. Bringing Xcode in from the Mac works with nobody clicking; the sizes and times are in the measures. The computer is kept as the Xcode computer for tests 24 to 30. |
| `signature-refused` | codesign or Gatekeeper refused the copied Xcode in the computer. A copy over SSH loses something (an extended attribute, a signature seal), so Silo must import the .xip instead and expand it inside. |
| `first-launch-failed` | Xcode passed the signature check but its license or first launch failed or waited on the screen over SSH. xcode-in-a-computer needs a step on the computer's screen, or another way to finish first launch; the sentence says which step. |
| `runtime-refused` | Xcode is ready but the runtime download failed in the computer (the error is in the sentence). If it asks for an account, xcode-in-a-computer must bring the runtime in from the Mac too. |
| `inconclusive` | Something else went wrong; the sentence and the attached logs say what. |

## 24. A simulator booted and used over SSH, with and without a user on the screen

Over SSH, does a simulator in a computer boot, install and launch an app, and give a screenshot that is not blank, first as the computer starts, then after its user is logged in on its screen?

- Decides: computers-macos-screen, computer-image (whether main logs its user in on its screen), simulators-for-parallel-agents
- Tier: `vm`. Step: after the first build. Time: about 25 minutes. Needs: the Xcode computer of test 23, macOS 27 on the Mac.
- Script: [`tests/24-simulator-over-ssh.sh`](tests/24-simulator-over-ssh.sh)

| Outcome | What it means |
|---|---|
| `works-without-screen-login` | Everything worked over SSH with nobody logged in on the computer's screen. Main needs no login on its screen for simulators; the boot time and the screenshot are in the measures. |
| `needs-screen-login` | It failed with nobody on the screen and worked once the user was logged in on it (automatic login, then a restart). Main must log its user in on its own screen at every start, and computer-image must set that up. Run 25 and 27 after this one: they log the user in the same way first. |
| `already-on-screen` | The provisioned computer logged its user in on its screen by itself, and everything worked. Main gets the session simulators need with nothing to set up. |
| `blank-screenshot` | The app ran but every screenshot was blank (one colour). The guest's graphics do not render the simulator; agents cannot check their work by looking at it. |
| `fails` | It failed in both cases; the step and error are in the sentence. |
| `inconclusive` | Something else went wrong; the sentence and the attached logs say what. |

## 25. How many booted simulators fit in one computer

In a computer with its user on the screen, how much memory does each booted simulator with an app running take, and how many boot before the computer starts to swap or a boot fails?

- Decides: simulators-for-parallel-agents (the maximum booted at once), sizing-target (memory of main for an iOS developer)
- Tier: `long`. Step: after the first build. Time: about 40 minutes. Needs: the Xcode computer of test 23, macOS 27 on the Mac; KIT_SIMS sets the most simulators booted (8).
- Script: [`tests/25-simulators-per-computer.sh`](tests/25-simulators-per-computer.sh)

| Outcome | What it means |
|---|---|
| `measured` | Simulators were booted one at a time until the computer swapped, a boot failed or the limit of the run was reached. The memory each one added and the number that fit are in the measures; the maximum of simulators-for-parallel-agents is set from them. |
| `first-boot-failed` | Not even one simulator booted; see test 24 first. |
| `inconclusive` | Something else went wrong; the sentence and the attached logs say what. |

## 26. codesign in a computer, over SSH, with no dialog

Over SSH, does codesign sign with an identity in the login keychain, and with one in a keychain of its own set up the way CI does it, without a dialog on the screen?

- Decides: apple-keys-in-a-computer (whether a development identity in the computer works for agents with nobody at the screen)
- Tier: `vm`. Step: after the first build. Time: about 15 minutes. Needs: the Xcode computer of test 23, macOS 27 on the Mac; no Apple account (a throwaway self-signed identity is made in the computer).
- Script: [`tests/26-codesign-over-ssh.sh`](tests/26-codesign-over-ssh.sh)

| Outcome | What it means |
|---|---|
| `both-sign` | Both keychains signed over SSH. Nothing special is needed for a development identity in a computer. |
| `own-keychain-only` | The login keychain failed or waited, and a keychain of its own, unlocked in the same session with its partition list set, signed. Silo's Apple development piece sets that keychain up; the fact login_after_unlock says whether unlocking the login keychain with the user's password would have been enough. |
| `neither` | Neither signed; the errors are in the sentence. errSecInternalComponent means the keychain was locked; a time-out means a dialog waited on the screen. |
| `inconclusive` | Something else went wrong; the sentence and the attached logs say what. |

## 27. Which certificates an app in a simulator trusts

Does an app in a simulator trust a certificate authority that the computer's system trusts, and does it after `simctl keychain add-root-cert`?

- Decides: registry-traffic-read and secrets-held-outside (whether Silo's certificate must be added to every simulator), simulators-for-parallel-agents (a step for each simulator made)
- Tier: `vm`. Step: after the first build. Time: about 20 minutes. Needs: the Xcode computer of test 23, macOS 27 on the Mac.
- Script: [`tests/27-certificate-in-simulators.sh`](tests/27-certificate-in-simulators.sh)

| Outcome | What it means |
|---|---|
| `separate-store` | The app refused the server before, still refused it once the computer's system trusted the authority, and accepted it after add-root-cert. Every simulator Silo or an agent makes needs Silo's certificate added to it. |
| `shares-system-store` | The app accepted the server once the computer's system trusted the authority. Nothing more is needed for simulators. |
| `system-trust-refused` | Adding the authority to the computer's system store failed over SSH (as test 21 may find); the simulator step was still tried and its result is in the sentence. |
| `never-trusted` | The app refused the server even after add-root-cert. |
| `inconclusive` | Something else went wrong; the sentence and the attached logs say what. |

## 28. App Store Connect tools through a boundary that re-signs their tokens

With the real App Store Connect key kept on the Mac and a placeholder key in the computer, do notarytool, altool and xcodebuild -allowProvisioningUpdates work through a boundary that ends TLS and signs each token again with the real key, also when each GET token is scoped to its one request, and which hosts, audiences and token lifetimes do they use?

- Decides: signing-keys-re-signed, apple-keys-in-a-computer (whether an App Store Connect key can stay out of every computer)
- Tier: `guided`. Step: after the first build. Time: about 30 minutes, with you at the keyboard for the key's details and perhaps one approval in the computer's window. Needs: the Xcode computer of test 23, macOS 27 on the Mac, Xcode on this Mac, an App Store Connect API key (.p8) on this Mac with its key ID and issuer ID (KIT_ASC_KEY, KIT_ASC_KEY_ID, KIT_ASC_ISSUER, or asked), network; for xcodebuild, KIT_ASC_PROJECT and KIT_ASC_SCHEME (a project of yours, copied into the computer).
- Script: [`tests/28-app-store-connect-resigned.sh`](tests/28-app-store-connect-resigned.sh)

| Outcome | What it means |
|---|---|
| `works-scoped` | Every tool tried worked through the boundary, also with each GET token scoped to its one request: signing-keys-re-signed can be built as proposed, with the narrowing; the hosts and audiences it must handle are in the facts. |
| `works-unscoped-only` | Every tool worked when its token was re-signed, but a GET token scoped to its request was refused by Apple: the boundary re-signs without a scope, and its own rule on method and path is the only narrowing. |
| `some-tools` | Some tools worked and others did not; the facts name, for each, the hosts it called, the audience and lifetime of its token and why it failed (a host outside the list, a longer lifetime, a refused certificate). signing-keys-re-signed handles the ones that worked first, and its notes say what the others need. |
| `certificate-refused` | The tools refused the boundary's certificate even though the computer's System keychain trusts its authority (they pin Apple's): no boundary can read or re-sign their traffic, and the key must be handled another way (Apple's cloud signing, or the tool run on the Mac). |
| `none-work` | The boundary saw and re-signed the tools' tokens but none worked; the statuses and errors are in the facts. Read them before changing signing-keys-re-signed. |
| `inconclusive` | The key did not work from this Mac itself, the computer's requests never reached the boundary, or its authority could not be trusted; the sentence says which. |

## 29. Xcode's agent bridge used over SSH, and who answers its approval

With Xcode open on the computer's screen, can an agent reached over SSH use `xcrun mcpbridge` (Xcode's tools for outside agents), and when Xcode asks to allow it, can a script inside the computer answer, or only a person, and is the answer kept for the next agent process?

- Decides: computers-macos-screen (whether main's screen must carry Xcode for agents), agents-drive-the-screen, xcode-in-a-computer (a step Silo's setup takes)
- Tier: `guided`. Step: after the first build. Time: about 25 minutes, with you at the computer's window for one or two clicks. Needs: the Xcode computer of test 23 with Xcode 26.3 or later, macOS 27 on the Mac.
- Script: [`tests/29-xcode-agent-bridge.sh`](tests/29-xcode-agent-bridge.sh)

| Outcome | What it means |
|---|---|
| `works-no-prompt` | mcpbridge listed Xcode's tools over SSH with nothing to approve: agents in main use Xcode's tools as soon as Xcode is open on main's screen. |
| `script-answers-prompt` | Xcode asked, and a script inside the computer (System Events, over SSH) clicked Allow: Silo's setup, or an agent driving the screen, can answer the prompt with nobody at the Mac. |
| `approved-once` | Xcode asked, a script could not answer, a person's Allow worked, and the next agent process was not asked again: one click per computer, which Silo's setup can ask for once, on the computer's screen. |
| `prompt-every-process` | Xcode asked again for each new agent process, and only a person could answer: on main this prompt is on main's screen, not the Mac's, but agents still need someone (or an agent driving the screen) at every start. |
| `no-bridge` | mcpbridge is missing or listed no tools even with a person at the screen; the facts hold its errors (an older Xcode, or Xcode not running). |
| `inconclusive` | Xcode did not open on the computer's screen, or nobody answered. |

## 30. An iPhone passed through USB to a computer

On macOS 27, does an iPhone given to a computer with USB passthrough show in the computer, pair with Xcode's device tools there, and take an install from it?

- Decides: builds-on-your-iphone (whether an agent's build reaches your iPhone from inside the computer, or leaves it for the Mac)
- Tier: `guided`. Step: after the first build. Time: about 20 minutes, with you at the keyboard to plug in and unlock the iPhone and tap Trust. Needs: the Xcode computer of test 23, macOS 27 on the Mac, an iPhone and its cable, and a VM helper with --usb-device (VZUSBPassthroughDevice), which ./kit build does not make yet; while the computer runs, the iPhone is the computer's and not the Mac's.
- Script: [`tests/30-iphone-usb-passthrough.sh`](tests/30-iphone-usb-passthrough.sh)

| Outcome | What it means |
|---|---|
| `attaches-and-installs` | The computer saw the iPhone, devicectl paired with it, and an install reached the phone (it installed, or the phone refused only the build's signature, which a development identity fixes): an agent's build can go to your iPhone from inside the computer, with the phone handed to that computer while it runs. |
| `attaches-not-paired` | The computer saw the iPhone on USB, but devicectl did not list it or could not pair: passthrough is not enough for Xcode, and builds-on-your-iphone takes the Mac route (the app comes out to the Mac and is installed from there). |
| `not-attached` | The computer never saw the iPhone, as reported on the macOS 27 beta: builds-on-your-iphone takes the Mac route. |
| `attach-refused` | The framework refused to attach the iPhone (the error is in the sentence): builds-on-your-iphone takes the Mac route. |
| `inconclusive` | No iPhone was found on this Mac, or the computer did not start; read the facts. |

## 31. The checkpoint engine's prototype on APFS: full, cbw, nbd, delta, and the catalog under kill

On APFS, does cloning a file that shares blocks with the live disk image (the last checkpoint, which nobody writes) make the live image's writes wait, as it did on XFS (up to 1.0 s), and how do the prototype's ways compare there?

- Decides: checkpoint-per-system (which way a checkpoint is taken on a Mac: a clone, cbw, or delta), checkpoint-at-each-turn, thinning-checkpoints (the wait of a drop), research 2026-11-24-checkpoint-engine
- Tier: `long`. Step: needed for step 4 of the first build. Time: about 50 minutes (four runs of an 8 GiB image, then the catalog killed at 300 steps). Needs: 40 GB free, no VM, no sudo; Zig 0.15.2 (downloaded from ziglang.org, about 51 MB, if not on PATH) and the prototype's source (fetched from this repository's branch if not next to the kit); both asked first.
- Script: [`tests/31-checkpoint-engine-on-apfs.sh`](tests/31-checkpoint-engine-on-apfs.sh)

| Outcome | What it means |
|---|---|
| `full-free` | Even a clone of the live image kept every write under 0.1 s: the plan's checkpoint (a clone) holds on APFS at this fragmentation; cbw or delta are needed only for the list of changed blocks, not for the wait. Check the pieces in the sentence against test 03 before relying on it. |
| `shared-clone-free` | A clone of the live image made writes wait, but cloning a checkpoint that shares blocks with it did not (0.1 s or less): cbw works on a Mac as on the prototype, through Silo's NBD server for a macOS computer (test 32); one copy of each block per checkpoint, no deltas to merge. |
| `shared-clone-waits` | Cloning a checkpoint that shares blocks with the live image made writes wait 0.1 to 1 s, as cbw did on XFS: cbw costs that wait at each checkpoint; prefer delta (named in the sentence with its own waits) wherever the wait is felt, as with checkpoint-at-each-turn. |
| `shared-clone-stalls` | Cloning a checkpoint that shares blocks with the live image made writes wait more than 1 s, as on XFS at 1.0 s: on a Mac, nothing that shares blocks with the live image may be cloned while it runs; take checkpoints as delta. |
| `not-a-moment` | A checkpoint on APFS did not hold exactly the writes up to one moment: the prototype's way is wrong on APFS; nothing is built on it until the cause is found (the run's files are attached). |
| `catalog-broke` | The catalog, killed at each of 300 steps on APFS with F_FULLFSYNC and RENAME_SWAP, did not recover, or a planted bug was not caught: the catalog's design needs another look before a Mac relies on it. |
| `inconclusive` | The prototype did not build or a way did not run; look at the attached logs. |

## 32. Does a macOS computer boot and run from a disk Silo serves over NBD

Does a macOS guest boot from its disk served over NBD (VZNetworkBlockDeviceStorageDeviceAttachment, macOS 14) by Silo's prototype server, what does a small flushed write cost inside it against the same disk attached as a file, and does a checkpoint taken through the server while the guest writes hold every flushed write and boot?

- Decides: checkpoint-per-system (whether a macOS computer's checkpoint can be taken from Silo's own log of writes: cbw or delta behind an NBD server), checkpoint-at-each-turn, research 2026-11-24-checkpoint-engine ("Who can keep the bitmap")
- Tier: `vm`. Step: needed for step 4 of the first build. Time: about 45 minutes (five starts of a computer). Needs: the base computer of test 00 (raw disk), 40 GB free, no sudo; the prototype's server if test 31 built it (~/SiloKit/bin/ckpt), else only the kit's Python server.
- Script: [`tests/32-boot-from-nbd.sh`](tests/32-boot-from-nbd.sh)

| Outcome | What it means |
|---|---|
| `boots-cheap` | The guest booted from the NBD disk within 1.5 times the file's time to SSH, a flushed 4 KiB write cost at most twice the file's, and the checkpoint taken through the server held every flushed write and booted: a macOS computer can have its disk served by Silo, so its checkpoints can be cbw or delta (test 31 says which) with no clone of the live image. |
| `boots-slow` | The guest booted from the NBD disk and the checkpoint held, but its start or its small writes cost more than that (numbers in the sentence): the NBD route works at a price; weigh it against the clone's wait of test 03 before choosing it for main. |
| `checkpoint-broke` | The guest booted from the NBD disk, but the checkpoint taken through the server lost a flushed write or did not boot: the server's moment is wrong under a real guest; nothing is built on it until the cause is found. |
| `no-boot` | The guest did not boot from the NBD disk (the client's errors and the server's are in the sentence): Silo cannot see a macOS computer's writes; its checkpoints stay clones of the live image (checkpoint-per-system as planned, with test 03's wait). |
| `inconclusive` | The computer did not start from its own file, or a measure is missing; look at the logs attached. |

## 33. Does the guest's TRIM free the blocks of the disk image on the Mac

When a macOS guest writes then deletes a few GB, does the disk image on the Mac shrink back (the guest's TRIM reaching the image as a hole), at once, after a sync and a minute, only when the volume is mounted again, or never; for the main raw image, a raw extra disk and an ASIF extra disk?

- Decides: checkpoint-per-system and thinning-checkpoints (whether a macOS computer's deleted files keep growing its checkpoints), checkpoint-storage, research 2026-11-24-checkpoint-engine ("TRIM")
- Tier: `vm`. Step: needed for step 4 of the first build. Time: about 30 minutes (two starts of a computer, 9 GB written in it). Needs: the base computer of test 00, 40 GB free, no sudo on the Mac; ASIF needs macOS 26.
- Script: [`tests/33-trim-reaches-image.sh`](tests/33-trim-reaches-image.sh)

| Outcome | What it means |
|---|---|
| `trim-reaches` | A raw image gave back at least 75% of what the write took within a minute of the delete and a sync: what a macOS computer deletes stops costing disk; a checkpoint keeps only what was there when it was taken, and the space model of the engine's research holds as written. |
| `trim-at-mount` | A raw image gave back at least 75% only after the volume was mounted again or the computer restarted: deleted files cost disk until the next start; Silo can count on the space coming back at each start (and thinning sees it then). |
| `asif-only` | Only the ASIF image gave space back: main's disk should be ASIF where space matters; a raw image keeps what was deleted. |
| `partial` | Between 25% and 75% came back: the guest trims some of what it frees; the sentence says how much per image and when. |
| `no-trim` | Less than 25% came back on every image, even after a restart: every block a macOS computer ever wrote stays in its image, so each daily checkpoint keeps what the guest's file system moved or deleted; the engine's space numbers are a floor, and Silo needs another way to free it (the NBD route of test 32, where Silo sees each TRIM, or zeroing free space inside the guest). |
| `inconclusive` | A write did not grow its image or a step failed; look at 33-alloc.csv and the logs. |

## 34. A night of Claude Code and Codex through a boundary that reads them, against an hour without it

Signed in with their plans in a macOS computer and left to work for a whole night through a boundary on the Mac that ends TLS and reads every call, are both programs' conversations recognised whole, are the plan's headers read, is every reply that is not a normal one kept as a fact, does no sign-in reach a file, and do the programs fare as well as in an hour without the boundary?

- Decides: model-traffic-read, recognising-a-conversation, provider-sees-silos-connection, reading-each-agent-program (the night it leaves to find out)
- **Blocks: option A of the first build (#373): its step 6 is done when a night through the boundary is clean, and this is that night with a stand-in for Silo's boundary.**
- Tier: `long`. Step: needed for step 6 of the first build. Time: about 10 hours: 8 through the boundary (KIT_34_HOURS), 1 without it (KIT_34_CONTROL_HOURS), and the set-up with you at the keyboard for the sign-ins (about 10 minutes). Needs: the installed computer of test 00, macOS 27 on the Mac, network, Claude Code signed in with a Claude plan (Max) and Codex with a ChatGPT plan (either one is enough), you at the keyboard at the start, the Mac on power and left awake (the test holds a caffeinate assertion); test 21's outcome, when it ran, chooses how the programs trust the boundary.
- Script: [`tests/34-night-through-the-boundary.sh`](tests/34-night-through-the-boundary.sh)

| Outcome | What it means |
|---|---|
| `night-clean` | Every model call of each program carried the one conversation id the program itself named, through its resumes (none without an id, none with two), the plan's headers were in the replies (anthropic-ratelimit-unified-*; Codex's x-codex-* or its rate-limit events), every reply that was not normal is a fact, no sign-in value is in any file, and the programs' own errors and retries were no worse than without the boundary. model-traffic-read and recognising-a-conversation (joined by the program's session) hold on a real night of plan traffic; provider-sees-silos-connection: the providers treated the boundary's connection as the program's own; option A's step 6 has its night, to run again with Silo's own boundary in place of this stand-in. |
| `conversations-split` | Some model calls carried no conversation id, or two that disagree, or the traffic named a conversation the program never named, or one it named never showed; the facts say which, for which program and how often. recognising-a-conversation's join by session does not hold as it stands for that program, and option A's receipts per conversation would be wrong there: the reader is fixed (a header or field this test did not know) before step 6 is called done. |
| `headers-missing` | Conversations were whole, but one program's replies carried no plan headers (no anthropic-ratelimit-unified-* for Claude Code, no x-codex-* header and no rate-limit event for Codex): option A shows that plan's share as not known, and spend at API prices only (#351); model-traffic-read reads the share from somewhere else or drops it. |
| `differs-from-control` | Through the boundary the programs' own errors and retries were clearly more frequent than without it (over twice the control's rate an hour plus 2, or a quarter more failed launches), a provider answered with a challenge, or a program refused the boundary's certificate. provider-sees-silos-connection is in question: the facts say what differed; before option A ships, find whether it is the boundary's connection (TLS, HTTP/1.1 only, this test's SSH hop) or the hour, with a second night. |
| `credential-leaked` | A sign-in value of the computer (~/.claude/.credentials.json, the keychain item, ~/.codex/auth.json) or a secret the boundary saw in a request was found in a file this run wrote; the facts name the files. Do not pack or share these results until those files are deleted. The stand-in's redaction failed, and Silo's boundary must pass the same search before step 6 is done. |
| `inconclusive` | Neither program was signed in, the computer's requests never reached the boundary, no model call went through it, or the control did not run; the sentence says which. |

## 35. Silo's gvproxy (#363's seal, grown in #380) with a macOS computer: what root inside reaches, sealed and closed

With #380's patch at gvisor-tap-vsock 197eea05, started with -sealed, does gvproxy give a macOS computer what it needs (a lease, DNS, the internet, Silo's own SSH forward) while nothing inside, root included, reaches the Mac (its loopback, its own addresses, gvproxy's API, the engine's socket) or the Mac's local network, by TCP or by ping? Started with -closed, does nothing go out at all (no connection, no ping, no name asked of a resolver) while Silo's SSH forward still gets in? How fast is traffic into the computer over the datagram socket, at MTU 1500 and 65520?

- Decides: network-stack (the sealed gvproxy as main's stack: #363's recommendation, #380's patch, #370's condition for a closed network and for "never your Mac, your local network"), main-on-macos-first (option A's stack)
- **Blocks: option A of the first build (#373): main is on Silo's network stack from its first start, and #363 and #380 recommend the sealed gvproxy as that stack until Silo writes its own.**
- Tier: `vm`. Step: needed for step 2 of the first build. Time: about 40 minutes, plus building gvproxy once (Go 1.26, about 65 MB, and gvproxy's modules). Needs: the base computer of test 00, git (the Command Line Tools), a 65 MB download of Go unless Go 1.26.0 is installed, no sudo on the Mac.
- Script: [`tests/35-sealed-gvproxy.sh`](tests/35-sealed-gvproxy.sh)

| Outcome | What it means |
|---|---|
| `sealed-holds` | Sealed, the computer got its lease, DNS, the internet and Silo's SSH forward, and root inside reached nothing of the Mac or its network: no loopback through 192.168.127.254, no port of the gateway but DNS, no API, no service or ping on the Mac's own address, no device on the Mac's network. Closed, nothing went out and SSH still got in. network-stack: the sealed gvproxy can carry main from its first start (option A), the first run can say "never your Mac, your local network" (#370 finding 3) without Apple's NAT, and the closed network #370 says Apple's NAT cannot give exists. Names still reach the Mac's resolver when sealed (the facts say so): a closed computer, or the boundary (#380), is what stops a name carrying data out. |
| `sealed-leaks` | Root inside reached something of the Mac or its local network when sealed, or anything outside when closed (named in the sentence): the patch misses a path on macOS; the sealed gvproxy cannot carry main until it is fixed, and the facts say what to add. |
| `sealed-breaks` | The sealed build gave no lease, no DNS, no internet or no SSH forward, or the closed one no SSH forward, while the same computer has them on gvproxy as released (test 07): the patch refuses something the computer needs; the sentence says what. |
| `inconclusive` | Silo's gvproxy did not build, or the computer did not start on it; the sentence says why. |

## 36. What a computer's connections meet on gvproxy and on Apple's NAT, and a restart of the stack

Through gvproxy, against Apple's NAT as the reference, does a computer's connection carry bytes exactly (100 bodies of 1 KB to 2 MB), pass a streamed reply's first event at once, keep a reply silent for 60 s and 330 s, and carry lines both ways on one connection? And when gvproxy restarts under a running computer, does the computer's network come back by itself, or only when the program holding the computer's network link connects again (a helper), or not at all?

- Decides: network-stack (#347's checks 6 to 10 against main's network, which #370 step 2 and the entry ask to be measured), when-the-engine-stops and #373 step 3 (a restart of the stack, or of the boundary in it, in the night)
- Tier: `vm`. Step: needed for step 2 of the first build. Time: about an hour (three starts of one computer, two replies held silent for 60 s and 330 s). Needs: the base computer of test 00, a 27 MB download of gvproxy (test 07's), the Mac on a network (its own address is where the computer connects), no sudo.
- Script: [`tests/36-stack-cost-and-restart.sh`](tests/36-stack-cost-and-restart.sh)

| Outcome | What it means |
|---|---|
| `transparent` | Every check gave the same through gvproxy as through Apple's NAT, and after gvproxy restarted, the computer's new connections worked again within 30 s with nothing done to the computer: the stack can be a process of its own, restarted in the night (#373 step 3), and connections open at the restart end, as the facts say. |
| `restart-needs-helper` | The checks held, but after gvproxy restarted the computer's network stayed dead until the program between the computer and the stack (here a relay standing in for Silo's helper) connected again and said who it is: the computer's network link must be held by the helper, which reconnects, never handed by the Virtualization framework straight to the stack's socket (as silovm and vfkit do). |
| `restart-breaks` | The checks held, but after gvproxy restarted, the computer's network did not come back even through the relay: a restart of the stack costs a restart of main; the stack must never stop while main runs (when-the-engine-stops, #373 step 3). |
| `alters` | A check gave less through gvproxy than through Apple's NAT (named in the sentence: bytes changed, the first event held back, a silent reply cut, lines lost): network-stack's cost is not only speed; gvproxy needs that fixed before it carries main. |
| `inconclusive` | The computer did not start on gvproxy, the Mac has no network address, or a check did not run; the sentence says which. |

## 37. The Mac sleeps and wakes with main running

While a computer runs, does its process hold the Mac awake by itself, and when the Mac sleeps anyway, does the computer come back whole after the wake: still running, answering SSH, a run left in tmux or screen still going, its clock right within a minute, its network working? How long was the sleep, as the Mac's clocks tell it and as the computer saw it?

- Decides: mac-kept-awake (does Apple's framework hold the Mac awake while a computer runs, without Silo), who-made-each-change (a sleep measured from the Mac's side as #370 writes it, with mach_continuous_time), runs-outlive-the-terminal (a run left detached keeps going across a sleep)
- Tier: `guided`. Step: needed for step 3 of the first build. Time: about 25 minutes; you put the Mac to sleep and wake it (with --allow-sudo the kit wakes it by itself after 3 minutes). Needs: the installed computer of test 00, macOS 27 on the Mac, network; the Mac may sleep (close nothing else that matters).
- Script: [`tests/37-sleep-and-wake.sh`](tests/37-sleep-and-wake.sh)

| Outcome | What it means |
|---|---|
| `wakes-whole` | The computer ran on after the wake: SSH, the detached run, the network, and its clock within 5 s of the Mac's a minute after the wake. A sleep pauses main and costs nothing else; the record writes it with author device (#370), and the helper measures it as the facts give. |
| `clock-behind` | The computer came back, but its clock was still behind the Mac's by more than 5 s a minute after the wake: main needs its clock set after each wake (the helper, or sntp in main), or every time in the record after a sleep is wrong. |
| `network-lost` | The computer came back, but its network did not within a minute of the wake. |
| `run-lost` | The computer stopped, stopped answering SSH, or the run left detached ended across the sleep; the sentence says which. A sleep must be refused while main runs (mac-kept-awake on battery too). |
| `no-sleep` | The Mac did not sleep (the clocks saw no sleep over 30 s); the fact vm_holds_awake says whether the computer's process held it awake. |
| `inconclusive` | The computer did not answer before the sleep, or a measurement is missing. |

## 38. The Mac's volume fills under a running computer

When the volume holding a running computer's disk image fills, what does the computer see (write errors, writes that wait, or nothing), does Apple's framework stop it, and once room is made, do its writes work again on a volume that still verifies?

- Decides: space-the-engine-keeps (whether a full volume stops the computer, damages it, or only fails its writes until room is made), low-disk and checkpoint-storage (what a full volume does to a running computer, which Silo's levels are set against)
- Tier: `vm`. Step: needed for step 4 of the first build. Time: about 30 minutes. Needs: the installed computer of test 00, macOS 27 on the Mac, 3 GB free (a 2 GB volume of its own, in a disk image the kit makes and deletes), no sudo on the Mac.
- Script: [`tests/38-full-volume.sh`](tests/38-full-volume.sh)

| Outcome | What it means |
|---|---|
| `io-error-recovers` | The computer's writes failed while the volume was full, the computer kept running, its writes worked again within a minute of room being made, and its volume verified: a reserve file freed when the volume fills is enough; Silo stops checkpoints at the floor and says why. |
| `writes-wait` | The computer's writes hung while the volume was full (no error, no progress), and went on by themselves once room was made: the volume verified. The engine still frees its reserve at once, since a computer whose writes hang looks dead to its agents. |
| `framework-stops` | Apple's framework stopped the computer when the volume filled (the run ended, or the computer stopped answering): the engine must keep room before the volume fills (reserve-file), never after. |
| `guest-damaged` | The computer kept running, but its volume did not verify after room was made: a full volume can damage main; the engine must never let it fill (reserve-file and the floor). |
| `inconclusive` | The volume did not fill within 10 minutes, or the computer did not start with the extra disk; the sentence says which. |

## 39. The contents of compressed files, read from outside the computer

Inside a macOS computer, files compressed by APFS (decmpfs: zlib, LZVN, LZFSE, in the xattr or in a resource fork) are common: the system's own apps keep them, and ditto writes them. Read from outside, from the computer's disk image, does a reader built on Apple's open-source decoders give back every file byte for byte, and which compressions does main actually hold?

- Decides: putting-back-a-path and file-changes-from-outside (#370 finding 6: look and put-back need the contents of a path, APFS compression included, from a reader no trial has run)
- Tier: `vm`. Step: needed for step 5 of the first build. Time: about 25 minutes (one first start, a few hundred MB copied inside, the disk read from the Mac). Needs: the installed computer of test 00 (a raw disk, FileVault off inside, as test 14 checks), no sudo on the Mac.
- Script: [`tests/39-compressed-files-from-outside.sh`](tests/39-compressed-files-from-outside.sh)

| Outcome | What it means |
|---|---|
| `reads-all` | Every file of the corpus read from outside equals what the guest read, compressed ones included (the sentence counts each compression): the second mode of #370's reader works as specified, with zlib and Apple's lzfse library; look and put-back can read a checkpoint without mounting it. The census of main's own data volume (facts) says how much of it is compressed. |
| `type-unread` | Every file the reader could decode was right, but main holds a compression it has no decoder for (named, LZBITMAP most likely): Silo's reader needs that decoder, or shows those files as a gap in look and put-back. |
| `reads-wrong` | A file read from outside differs from the guest's, a file the guest marks compressed was read as plain, or a plain file gave a gap: the reader's handling of that case is wrong (examples in the facts); nothing reads contents from outside until it is fixed. |
| `inconclusive` | The computer did not start, its disk is not raw, its data volume is encrypted, or no compressed file was made; the sentence says which. |

## 40. A network card that leads nowhere, with Silo's relay and proxy over vsock (#378)

With a network card whose other end on the Mac answers ARP for one router and drops every other frame, and a relay that carries 127.0.0.1:3128 inside to Silo's proxy over vsock and the Mac's SSH in over vsock: does SSH get in; which programs (curl, git, npm, Node's fetch, pip, Claude Code, Codex, Homebrew, xcodebuild) reach the internet through the proxy with the system proxy set, with the proxy variables, with both and with neither; does anything reach anything without it, root included (a route, another address, ping, a name server, the Mac's own address); and is a macOS computer "offline" when the router's ARP goes unanswered?

- Decides: network-stack (#378's card as main's network, which #380 recommends for the first build), main-on-macos-first (option A without gvproxy), network-place (the frames the card drops)
- **Blocks: option A of the first build on #378's card instead of gvproxy, which #380 and #382 recommend for main's first start: then test 07 no longer blocks it.**
- Tier: `vm`. Step: needed for step 2 of the first build. Time: about an hour (a first start on Apple's NAT to put the relay in, then two starts on the card, 4 settings of 9 programs). Needs: the base computer of test 00, about 250 MB downloaded inside the computer on its first start (Node, Claude Code, Codex), the Mac on a network, no sudo on the Mac.
- Script: [`tests/40-card-that-leads-nowhere.sh`](tests/40-card-that-leads-nowhere.sh)

| Outcome | What it means |
|---|---|
| `closed-holds` | SSH came in over vsock, the proxy carried curl and at least one agent program, and nothing reached anything without the proxy: every attempt by root (a route, an address of its own, ping, a lookup at a public name server, the Mac's own address) ended at the card, which counted it. network-stack: #378's card can carry main's first build without gvproxy (#380's recommendation), and test 07 stops blocking option A. The facts say which programs need which proxy setting; a program that follows neither stays closed. |
| `leaks` | Something reached a server without the proxy (named in the sentence): the card or the relay lets something through that #378 says it cannot; the card cannot carry main until that is found. |
| `relay-fails` | SSH over vsock never answered, or nothing inside ever reached the proxy: the relay does not work in a macOS computer as built (AF_VSOCK, launchd or silovm's bridges); the sentence says which. #378's card cannot be main's network until it does. |
| `inconclusive` | The computer did not start or did not take the relay on its first start on Apple's NAT; the sentence says why. |

## 41. Ping and name lookups past gvproxy: as released, #363's patch and #380's

On a Mac, with a macOS computer on gvproxy (as released and hardened, with #363's -sealed, with #380's -sealed and with #380's -closed), does root inside ping the Mac's own address, the Mac's router and the internet; does a TCP connection to a service on the Mac get through; and does a lookup of a random name leave through the Mac's resolver (watched with tcpdump on the Mac)? Does Silo's own SSH forward still get in under each?

- Decides: network-stack (#380: #363's patch leaves ping open, and only #380's -sealed and -closed close it; a lookup is a way out unless -closed), network-before-rules (#348: never your Mac, your local network or cloud metadata)
- Tier: `vm`. Step: needed for step 2 of the first build. Time: about 40 minutes (four starts of one computer), plus building #380's gvproxy once (Go 1.26, as test 35). Needs: the base computer of test 00, git (the Command Line Tools), Go 1.26.0 (downloaded, about 65 MB, unless installed), the Mac on a network; sudo with --allow-sudo only, to watch the Mac's name lookups with tcpdump (otherwise that half is not watched).
- Script: [`tests/41-ping-and-names-past-gvproxy.sh`](tests/41-ping-and-names-past-gvproxy.sh)

| Outcome | What it means |
|---|---|
| `silo-holds` | Under #380's -sealed, no ping and no connection reached the Mac or its router; under -closed, nothing at all left: no ping, no lookup (refused inside, and not seen on the Mac's network when watched); SSH came in under all four. network-stack: #380's patch closes on macOS what #363's leaves open; the facts say whether #363's still pings the Mac, as #380 found on Linux. |
| `silo-leaks` | Under #380's -sealed or -closed, something reached the Mac, its router, or (under -closed) the internet or the Mac's resolver; the sentence names it. #380's patch misses a path on macOS. |
| `silo-breaks` | #380's build gave no lease or no SSH forward where gvproxy as released did; the sentence says which setting. |
| `inconclusive` | #380's gvproxy did not build, or the computer did not start on gvproxy as released. |

## 42. Throughput over the datagram socket a Mac gives gvproxy, at MTU 1500 and the largest the framework takes

Through gvproxy, which a Mac reaches only over a Unix datagram socket (one frame per datagram), how fast does a 512 MB download into the computer and a 256 MB upload out of it run, at MTU 1500, 9000 and the largest VZFileHandleNetworkDeviceAttachment takes, against the same over Apple's NAT? What are the Mac's datagram limits (net.local.dgram.maxdgram, recvspace)?

- Decides: network-stack (#380: over a datagram socket, traffic into the computer was 0.28 to 0.90 Gbit/s at MTU 1500 against 1.05 to 1.13 over a stream socket, cause not found; Apple's framework gives only the datagram socket), own-cost-targets
- Tier: `vm`. Step: needed for step 2 of the first build. Time: about 30 minutes (one computer started four times: Apple's NAT, then gvproxy at three MTUs). Needs: the base computer of test 00, a 27 MB download of gvproxy (test 07's), no sudo.
- Script: [`tests/42-datagram-throughput.sh`](tests/42-datagram-throughput.sh)

| Outcome | What it means |
|---|---|
| `fast-enough` | Into the computer through gvproxy at MTU 1500 is at least half of Apple's NAT: the datagram socket is not the limit for main; network-stack needs no MTU change for it. |
| `needs-large-mtu` | At MTU 1500 into the computer is under half of Apple's NAT, and at a larger MTU it is at least half: silovm and gvproxy set that MTU from the first start (the fact mtu_max says the largest the framework took). |
| `datagram-limits` | Into the computer stays under half of Apple's NAT at every MTU: the Mac's datagram transport is the limit, which #380 names as a case for a different stack; #378's card, which carries traffic over vsock, is not affected. |
| `inconclusive` | The computer did not start on gvproxy or on Apple's NAT, or a transfer failed; the sentence says which. |

## 43. How the first run reaches main before the relay is in it (#382)

A provisioned computer turns on SSH on its network card, and a card that leads nowhere gives SSH no road until the relay is in. Does any option of Apple's guest provisioning run a program at the first start (then the relay could start with no road at all)? How long does #382's recommended way take (first start on Apple's NAT, relay in over SSH, a stop, a start on the card, SSH over vsock), and what can the computer reach meanwhile? Does the second way (gvproxy with nothing out, SSH in through its forward) do the same?

- Decides: way-in-before-the-relay (#382's three ways for the first SSH, when main's card leads nowhere), first-run-time-macos (what the extra start costs)
- Tier: `vm`. Step: needed for step 2 of the first build. Time: about 40 minutes (two first starts, each followed by a start on the card); 5 minutes more to download gvproxy if test 07 has not. Needs: the base computer of test 00, the Command Line Tools (the SDK's headers), the Mac on a network, a 27 MB download of gvproxy (test 07's) for the second way, no sudo on the Mac.
- Script: [`tests/43-way-in-before-the-relay.sh`](tests/43-way-in-before-the-relay.sh)

| Outcome | What it means |
|---|---|
| `nat-first-start-works` | The first start on Apple's NAT took the relay, and the start on the card answered SSH over vsock; the measures give what the extra start costs and the facts what the computer could reach during it. way-in-before-the-relay: #382's recommendation can be built as written; the fact gvproxy_first_start says whether the second way is a working alternative. |
| `nat-first-start-fails` | The relay did not go in over Apple's NAT, or the computer did not come back over vsock after it did: the recommended way does not work as written; the sentence says where it stopped. |
| `inconclusive` | The computer did not start or answer SSH on its first start; the sentence says why. |

## 44. The browser Silo starts for a port preview, on a Mac: names, WebRTC, other computers, its own traffic

#384 ran Chrome for Testing headless on Linux. On a Mac, in a headed, branded Google Chrome started with #384's flags (its own profile, a rule proxy per computer, WebRTC only through the proxy, no name looked up by the browser) and a page that tries every way out #384 measured, does any name carrying the page's secret reach the Mac's resolver, does the page reach another computer's preview, and what does a branded Chrome send by itself? Without --host-resolver-rules (the control), does the TURN server's name leak as #384 found?

- Decides: ports-reached-from-your-mac (#384's set-up: a browser of Silo's own per computer, a rule proxy, --host-resolver-rules so the browser looks no name up itself)
- Tier: `guided`. Step: after the first build. Time: about 5 minutes; you look at Chrome's window once. Needs: Google Chrome in /Applications (or KIT_44_CHROME), no VM; sudo with --allow-sudo only, to watch the Mac's name lookups with tcpdump (without it, the lookups half is not watched and the outcome is inconclusive).
- Script: [`tests/44-preview-browser.sh`](tests/44-preview-browser.sh)

| Outcome | What it means |
|---|---|
| `flags-hold` | With #384's flags no name carrying the secret was looked up on the Mac (watched with tcpdump) and no request reached the other computer's preview: ports-reached-from-your-mac can take #384's set-up on a Mac; the facts list what Chrome sent by itself. |
| `flags-leak` | With #384's flags, a name carrying the secret reached the Mac's resolver, or the page reached the other computer's preview; the sentence names the way. The set-up needs another wall on a Mac. |
| `inconclusive` | Chrome did not load the page, or the lookups were not watched (no --allow-sudo); the facts hold what was seen. |

## 45. What a disk keeps of the record when its power is cut: F_FULLFSYNC against fsync

A writer appends numbered lines with a CRC to a file on a USB disk, forcing each out (F_FULLFSYNC, or plain fsync) and noting each line it would call safe on the Mac's own disk. The disk loses power mid-write. Once it is back: is every line acknowledged before the cut there and whole? Is any line damaged with whole lines after it (damage in the middle, which #386's recorder must handle on start), or only an unfinished end?

- Decides: record-damaged-at-start (#386: how often the recorder will start on a damaged line, other than an unfinished last one), record-speed-cost and engine-saved-state (whether F_FULLFSYNC, which test 06 prices, is what keeps an acknowledged line)
- Tier: `guided`. Step: needed for step 5 of the first build. Time: about 10 minutes per way of forcing a write (two by default); you pull a USB disk's cable while the kit writes, then plug it back. Needs: an external USB disk you can lose the contents of a folder on (its own power from the cable, so pulling the cable cuts it), named in KIT_45_VOLUME or the only external volume mounted; you at the keyboard; no sudo, no VM.
- Script: [`tests/45-record-power-cut.sh`](tests/45-record-power-cut.sh)

| Outcome | What it means |
|---|---|
| `full-keeps-acked` | With F_FULLFSYNC every acknowledged line came back whole and damage, if any, was only after the last of them: the record's writer can acknowledge a line once F_FULLFSYNC returns; the facts say what plain fsync kept on the same disk. |
| `full-loses-acked` | With F_FULLFSYNC, a line acknowledged before the cut is missing or damaged: this disk lies about its cache, and no writer on it can promise a line; Silo keeps the record on the Mac's internal disk only, and the facts say whether fsync did worse. |
| `inconclusive` | No external volume, the cable was not pulled while lines were written, or the volume did not come back. |

## 46. What deleting a checkpoint frees while a local Time Machine snapshot exists

On the volume Silo keeps checkpoints on, a clone shares blocks with the file it came from and a write to it takes new ones. Without a local snapshot, does deleting the clone and its file give their space back at once? Once a local Time Machine snapshot exists, how much does the same deletion free, what private size and clone id does APFS report for the files, and how much comes back when the snapshot goes?

- Decides: low-disk (#378: the level where Silo pauses computers must not count on space freed by removing checkpoints in the same minute), space-the-engine-keeps (a reserve the engine holds, since deleting may free nothing), checkpoint-storage (how Silo counts what a checkpoint holds)
- Tier: `quick`. Step: needed for step 4 of the first build. Time: about 5 minutes. Needs: 3 GB free on the volume of ~/SiloKit, the Command Line Tools (./kit build), no VM; it makes one local Time Machine snapshot of that volume (after asking), which macOS removes within 24 hours, and with --allow-sudo the kit removes it at the end.
- Script: [`tests/46-local-snapshots-and-free-space.sh`](tests/46-local-snapshots-and-free-space.sh)

| Outcome | What it means |
|---|---|
| `snapshot-holds-space` | With a local snapshot, deleting the files freed under a quarter of what they took, as #378 says, and the space came back when the snapshot went (or the kit could not remove it; the facts say). low-disk: Silo never counts on space freed by deleting checkpoints while a local snapshot exists, and shows "frees once macOS removes its local snapshots"; space-the-engine-keeps: the reserve file is the defence that holds. |
| `delete-frees` | Deleting freed at least three quarters of what the files took even with a local snapshot: #378's trap does not hold on this macOS; the facts give the private sizes APFS reported. |
| `no-snapshot` | macOS made no local snapshot (Time Machine refused it without sudo, or is turned off by a profile); only the control ran. Rerun with --allow-sudo. |
| `inconclusive` | The files could not be made, or free space moved too much for a reading (another program wrote meanwhile). |

## 47. Option B's Linux main through libkrun on this Mac: boot, stop, memory given back, disk speed (#381 L1 to L3)

On this Mac and its macOS, does libkrun on Apple's Hypervisor framework boot Ubuntu 26.04 to SSH over vsock with no network device, how long do the first and second boots take, is systemd "running"? Does libkrun's stop request (its shutdown eventfd, with the seed's HandleRebootKey=poweroff) power main off? After main touches and frees 4 GiB, how much does the Mac get back? How fast does main write to its disk, against the Mac's own?

- Decides: main-on-macos-first (option B, a Linux main first, #381), linux-vm-engine (#381's measurements 1, 4 and 5 on a Mac), mac-silo-needs (which macOS option B needs)
- Tier: `vm`. Step: needed for step 2 of the first build. Time: about 20 minutes, plus about 1 GB downloaded once (Ubuntu 26.04's arm64 cloud image, kernel and initrd). Needs: Homebrew's libkrun (brew tap slp/krun; brew install libkrun) and ./kit build after it, 8 GB free, no sudo, no macOS 27 (libkrun's floor is macOS 14, to confirm).
- Script: [`tests/47-linux-main-on-hvf.sh`](tests/47-linux-main-on-hvf.sh)

| Outcome | What it means |
|---|---|
| `boots-and-stops` | Main booted to SSH over vsock, systemd running, and libkrun's stop request powered it off: option B's first build runs on this macOS; the measures give its boot, the memory it gives back and its disk against the Mac's. |
| `stop-ignored` | Main booted, but the stop request did not power it off within 60 s: the helper needs another way to stop main (a poweroff over SSH, then a kill), which #381's step 2 must say. |
| `no-boot` | Main never answered SSH over vsock within 5 minutes: option B does not run on this Mac as specified; console.log says why. |
| `inconclusive` | The image could not be prepared, or a measurement failed. |

## 48. Option B's disk.raw on APFS: a clonefile while main writes, and discards (#381 L5, L8)

Main writes numbered 4 KiB blocks, each flushed before the next. Does every clonefile of disk.raw taken meanwhile hold a prefix of them (every block up to some n and none after), read through #381's ext4 reader with the journal replayed? After main writes 1 GiB, deletes it and runs fstrim, how much of disk.raw's space comes back to the Mac?

- Decides: linux-vm-engine (#381: a clone of a running main's disk as a checkpoint, under B), space-the-engine-keeps (whether a deleted file's space comes back to the Mac), main-on-macos-first (option B's checkpoints)
- Tier: `vm`. Step: needed for step 4 of the first build. Time: about 15 minutes, after test 47's image. Needs: Homebrew's libkrun and ./kit build, the image test 47 prepares, 10 GB free, no sudo.
- Script: [`tests/48-linux-disk-on-apfs.sh`](tests/48-linux-disk-on-apfs.sh)

| Outcome | What it means |
|---|---|
| `moment-and-trim` | Every clone held a prefix, and at least 900 MiB came back after fstrim: under B, a checkpoint is a clonefile taken while main runs, and space follows main's deletions. |
| `torn` | A clone held block n+1 without block n: under B a checkpoint needs a pause around the clone (#381's K07 question on APFS). |
| `trim-ignored` | Every clone held a prefix, but under 900 MiB came back: libkrun on a Mac does not punch holes for discards, so the engine keeps a reserve and #381's step 4 says disk.raw only grows. |
| `inconclusive` | Main did not boot (see test 47), or the reader failed. |

## 49. Option B's main run by a launch daemon while you are logged out (#381 L7)

When krun-boot runs main as a launch daemon in the system domain (as your user, with the hypervisor entitlement), does main start, and does it keep running, with its clock and its writes going on, while you log out and back in?

- Decides: engine-matches-wanted (computers that keep running while you are logged out), linux-vm-engine (whether libkrun's hypervisor use is allowed outside your login session)
- Tier: `guided`. Step: after the first build. Time: about 15 minutes; you log out for 3 minutes and log back in. Needs: Homebrew's libkrun and ./kit build, the image test 47 prepares, --allow-sudo (a launch daemon is installed and removed), you at the Mac.
- Script: [`tests/49-linux-main-logged-out.sh`](tests/49-linux-main-logged-out.sh)

| Outcome | What it means |
|---|---|
| `runs-logged-out` | Main started from launchd and wrote every 10 s while you were logged out: under B the engine can keep computers going with nobody logged in. |
| `stops-at-logout` | Main started, but its writes stopped while you were logged out (or it was gone when you came back). |
| `refused` | launchd could not start main (the hypervisor refused outside a login session, or the job failed); the facts say how. |
| `inconclusive` | The logout was skipped, or main did not answer before it. |

## 50. What Silo's own writers wait for on APFS: an acknowledgement with F_FULLFSYNC, F_BARRIERFSYNC or fsync

On this Mac's APFS, what does the record's writer wait for before it acknowledges a line, when every flush is F_FULLFSYNC, when only the flush before an acknowledgement is (the rest F_BARRIERFSYNC), and with plain fsync (which keeps nothing through a power cut), alone and while another program writes; and what does a SQLite commit cost with fullfsync?

- Decides: record-file-format (the flush before an acknowledgement), record-damaged-at-start (whether the barrier rule of research 2026-11-25-record-head-off-the-mac is worth it), what-a-budget-promises (the spend journal's two flushes per model call), pooled-budget-ledger (the team ledger's commit on a Mac), research 2026-11-25-durability-on-a-mac
- Tier: `long`. Step: needed for step 1 of the first build. Time: about 15 minutes (18 runs of the record's writer and 4 of SQLite, quiet and while another program writes). Needs: 2 GB free, no VM, no sudo; Zig 0.15.2 and the writer's source, as test 31 (both asked first).
- Script: [`tests/50-writers-flush-cost.sh`](tests/50-writers-flush-cost.sh)

| Outcome | What it means |
|---|---|
| `full-affordable` | With every flush F_FULLFSYNC, an acknowledgement waits 50 ms or less at the 99th percentile even while another program writes, new blobs included: Silo's writers flush that way everywhere, the spend journal's two flushes per model call cost nothing a person sees, and no barrier rule is needed. |
| `barrier-for-blobs` | Each acknowledgement alone is 50 ms or less, but a line with a new blob passes it with every flush F_FULLFSYNC and not with the barrier rule: the writer keeps F_FULLFSYNC before each acknowledgement and F_BARRIERFSYNC for blobs and folders, as #394's mac-barrier model, and test 51 must show the barrier holds. |
| `group-lines` | An acknowledgement with F_FULLFSYNC passes 50 ms while another program writes, and grouping 16 lines per flush brings the rate back over 1,000 lines a second: the writer groups what arrives while a flush runs (one flush acknowledges them all), as the recorder's batch already does. |
| `too-slow` | Even grouped by 16, F_FULLFSYNC leaves the writer under 1,000 lines a second while another program writes: the record's flush needs another design (a longer batch, or acknowledging after the flush is only issued) before the first build relies on it. |
| `inconclusive` | The writer did not build or a run failed; look at the attached logs. |

## 51. A power cut under the record's writer: a macOS computer on a disk that loses its cache

When a macOS computer loses power while the record's writer runs, with its disk served by a stand-in drive that keeps writes in a cache until a flush and loses part of that cache in the cut, does every line the writer acknowledged survive and the record start again, with every flush F_FULLFSYNC, with the barrier rule, and with plain fsync?

- Decides: record-file-format (the flush before an acknowledgement), record-damaged-at-start (the after-head start rule), research 2026-11-25-durability-on-a-mac, research 2026-11-25-record-head-off-the-mac
- Tier: `vm`. Step: needed for step 1 of the first build. Time: about 40 minutes (seven starts of a computer). Needs: the base computer of test 00 (raw disk), 40 GB free, no sudo; Zig 0.15.2 and the writer's source, as test 31 (both asked first).
- Script: [`tests/51-power-cut-through-the-disk.sh`](tests/51-power-cut-through-the-disk.sh)

| Outcome | What it means |
|---|---|
| `full-holds-fsync-loses` | With F_FULLFSYNC, and with the barrier rule, every acknowledged line survived every cut and the record started; with plain fsync some did not: the writer's flushes are right on APFS, and the barrier rule may be used where test 50 says it saves time. |
| `barrier-loses` | F_FULLFSYNC kept every acknowledged line, the barrier rule did not: F_BARRIERFSYNC does not order the writes the rule needs through this disk; the writer uses F_FULLFSYNC for every flush. |
| `fsync-holds` | Even plain fsync kept every acknowledged line: the computer's disk path flushed on its own (the flush counts are in the facts), so this stand-in cannot tell the flushes apart; trust #394's models and test 50 instead. |
| `full-loses` | An acknowledged line was lost even with every flush F_FULLFSYNC: F_FULLFSYNC does not reach the disk through this path, or the writer is wrong; nothing relies on the record's durability on a Mac until the cause is found (the runs are attached). |
| `no-start` | The computer did not start again after a cut: APFS itself did not survive this stand-in's power cut, which says more about the stand-in than about the writer; look at the attached logs. |
| `inconclusive` | The computer did not start from the served disk or the writer did not run; read the facts. |

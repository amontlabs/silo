# Main, the macOS computer: a second opinion on eight decisions

An agent's part, 24 November 2026, for polarzero. It edits nothing in `plan/`. Every fact below is
marked **verified** (I read the source, the document or ran the command) or **not verified**.
Everything about macOS here is from documents, forum threads and source code. This container has
no Mac, so no virtual machine was run.

| id | Recommendation | Confidence | One-line reason |
|---|---|---|---|
| `main-on-macos-first` | Take as written; correct three facts in its notes | high | The answer holds. The notes understate the download (25.6 GB, not "over 10 GB") and leave out Apple's purpose clause and the absence of Docker in main |
| `one-main-or-several` | Take as written | high | Apple's limit of two running macOS computers makes a second main take the only place left for a fork or a macOS task |
| `neutral-core` | Take with an edit | medium | The limit is not a number the driver knows: other apps take places, and a known bug leaks places until the Mac restarts. The core must take a refused start as a fact |
| `computer-image` | Split: take the macOS half with an edit, leave the Linux half open | high | macOS 27 sets up a new macOS computer from the framework, so what Silo keeps must be the installed system before its first start. The Linux half is not needed by the first build |
| `first-run-time-macos` | Take with an edit | high | The download is 25.6 GB: 34 minutes alone at 100 Mbit/s, not "more than ten". The answer should also check free disk first and keep the Mac awake |
| `files-through-a-channel` | Take with an edit | medium | The property as worded fails its own hostile test in the first build, where the network and the way in are channels nobody granted one by one. Name the channels |
| `pause-every-computer` | Take with an edit | medium | A paused macOS computer lives only in the process that holds it. A Mac restart turns a pause into a stop, and the answer does not say what "resume" then does |
| `what-stop-ends` | Take with an edit | medium | How Apple's framework asks a macOS guest to shut down is not documented, and a reported bug keeps one of the two places taken after a stop. Both are to measure on macOS too, not only on Linux |

## `main-on-macos-first`: Which system does main run first?

- **Recommendation:** Take as written. The answer is right, and the facts it rests on are stronger
  than the notes say, except for three that the notes get wrong or leave out (below).
- **Confidence:** high. It rests on Apple's license text, Apple's documentation and Apple's own
  engineers on its forums, all read.
- **What it decides, on one real case:** You install Silo. Main is macOS 27.2.1. Claude Code builds
  and tests the Silo app itself with Xcode in main. A Linux computer, when one exists, is made fresh
  and never copied from main.
- **Options:**
  - macOS first (the answer). Costs a 25.6 GB download, a macOS computer's memory, and at most one
    macOS computer beside main. Who does it: Tart (Cirrus Labs) and Anka (Veertu) run macOS guests
    for CI on Apple's framework; Orka and AWS EC2 Mac rent whole Macs for the same reason.
  - Linux first. Costs what no cloud sandbox lacks, and every Mac or iOS build.
- **Evidence:**
  - Apple's license for macOS Tahoe, section 2B(iii), word for word the same in Sequoia's
    (verified: I downloaded both PDFs from apple.com/legal/sla and compared the clause): "to install,
    use and run up to two (2) additional copies or instances of the Apple Software […] within
    virtual operating system environments on each Apple-branded computer you own or control that
    is already running the Apple Software, for purposes of: (a) software development; (b) testing
    during software development; (c) using macOS Server; or (d) personal, non-commercial use."
    It also excludes "service bureau, time-sharing, terminal sharing, relay service or other similar
    types of services". https://www.apple.com/legal/sla/docs/macOSTahoe.pdf
  - The framework enforces the two only for macOS guests. Linux guests have no fixed cap. Apple's
    DTS engineer, September 2026 (verified, read):
    https://developer.apple.com/forums/thread/845455 . The same thread quotes the macOS 27 license
    with a new opening, "except as otherwise provided in writing, signed, or issued by an authorized
    representative of Apple", and nobody at Apple could say how to ask for that.
  - The restore image of macOS 27.2.1 for a virtual Mac is 25,595,954,706 bytes (verified:
    `Content-Length` from Apple's CDN for `UniversalMac_27.2.1_26B131_Restore.ipsw`). macOS 26.x
    images were 18.3 to 19.8 GB, macOS 15.x about 16.8 GB (verified: ipsw.me's API, which lists
    Apple's URLs). The notes say "over 10 GB".
  - A macOS guest cannot run virtual machines. Nested virtualization exists only on
    `VZGenericPlatformConfiguration`, which is for Linux guests, on M3 and later (verified: Apple's
    documentation of `isNestedVirtualizationSupported`). So Docker cannot run inside main. The plan
    already says so in `containers-on-macos` ("as far as is known"); this confirms it.
  - A macOS 15 or later guest can sign in to iCloud, if it was installed from a macOS 15 or later
    image. Copies started at the same time get a new identity and must sign in again (verified:
    Apple, "Using iCloud with macOS virtual machines",
    https://developer.apple.com/documentation/virtualization/using-icloud-with-macos-virtual-machines).
  - `sudo` and Homebrew work in a macOS guest as on a Mac. Cirrus Labs' public images give the user
    passwordless `sudo` with one sudoers line and install Homebrew in their `base` image (verified:
    https://github.com/cirruslabs/macos-image-templates, `templates/vanilla-tahoe.pkr.hcl`).
- **Risks and what would change this:**
  - The purpose clause. An agent in main that does work other than software development, for a
    company, is outside (a) to (d). The first user is polarzero, building software, so it does not
    bite now. It bites `where-it-runs` ("a cloud of its own"): renting macOS computers is allowed
    only under section 3, which leases a whole Mac to one user for at least 24 hours (verified, same
    PDF). See the cross-cutting findings.
  - Main cannot run Docker. Many projects start Postgres in Docker for their tests. In the first
    build there is no Linux computer, so those tests cannot run anywhere in Silo. polarzero should
    check whether the projects polarzero will use first need Docker.
- **Corrections to the notes** (the answer is unchanged):
  - "It costs a first run that downloads macOS from Apple, over 10 GB" becomes "about 26 GB for
    macOS 27".
  - Add: "Apple's license allows the two macOS computers for software development, testing during
    it, macOS Server, or personal non-commercial use."
  - Add: "A macOS computer cannot run Docker: Apple's framework gives nested virtualization only to
    Linux guests."
- **For the first build:** Does not block. It is the premise of the build.

## `one-main-or-several`: Does a user have one main or several?

- **Recommendation:** Take as written. It closes a question that `main-and-its-agents` already
  answered, and Apple's limit adds a reason the notes do not give.
- **Confidence:** high. It rests on the license and the framework's limit above, both verified.
- **What it decides, on one real case:** You work for two clients. With two macOS mains, both of
  Apple's places are taken all day. You cannot start a macOS fork to try a tool, and a task's macOS
  half waits forever (`task-macos-limit`). With one main, the second place stays free.
- **Options:**
  - One main (the answer). Costs a client's separation: it goes in another computer.
  - Several mains. Costs the second macOS place, and every screen asks which main.
- **Evidence:** the two-instance clause and the framework's cap, as above (verified). The iCloud
  document above: each macOS computer has its own identity, so two mains are two Macs to iCloud
  (verified).
- **Risks and what would change this:** Apple lifting the cap of two. The macOS 27 license now
  mentions written exceptions, but no route to one exists (verified, thread 845455). A main on Linux
  for a second client would not hit the cap; that is a later question.
- **For the first build:** Does not block. It removes a "which main" argument from every command.

## `neutral-core`: Does the engine's core know which system a computer runs?

- **Recommendation:** Take with an edit. The design is right, but it treats Apple's limit as a
  number the driver can give. On a real Mac the driver cannot know that number, so the core must
  also take "the device refused to start it" as a fact.
- **Confidence:** medium. The bug below is reported by developers and acknowledged by an Apple
  engineer. Nobody has shown whether macOS 27 fixed it.
- **What it decides, on one real case:** Main runs. UTM, which polarzero also uses, runs one macOS
  virtual machine. Silo's count says one place is free, so it starts a fork, and Apple's framework
  refuses with `VZErrorVirtualMachineLimitExceeded`. The core must record the refusal and show who
  holds the places, not believe its own count.
- **Options:**
  - The core holds a limit that comes from the driver, and nothing else (the answer). Costs a wrong
    decision whenever the limit is shared with other apps.
  - The core holds the limit as the driver's best guess, and a start the device refuses becomes a
    fact that the computer waits on (proposed). Costs one more kind of fact.
  - The core knows macOS. Costs what the answer says.
- **Evidence:**
  - The limit counts every macOS virtual machine on the Mac, whichever app started it. Tart
    catches `.virtualMachineLimitExceeded` and lists the other running VMs it knows (verified:
    https://github.com/cirruslabs/tart/blob/main/Sources/tart/Commands/Run.swift).
  - Developers report that after a macOS guest shuts down, or after `tart stop`, the kernel's
    counter (`hv_apple_isa_vm_quota`) is not decremented, and starts fail until the Mac restarts.
    macOS 26.5, M4 Max, FB22967193. An Apple engineer said they are investigating; another user
    reproduced it in July 2026 (verified, read:
    https://developer.apple.com/forums/thread/830119). Whether macOS 27 fixes it: not verified.
  - Apple's limit error is "starting a VM would exceed the system's limit on the number of
    simultaneously running virtual machines" (verified, Apple's documentation of the error).
- **Risks and what would change this:** If the leaked place is fixed in macOS 27, the count is
  wrong only when other apps run macOS virtual machines, which is rarer. The edit still costs
  little and still holds.
- **Proposed text:** answer: "Nothing in the core names a system. The core holds a computer's
  system as a value it can only compare, so it refuses a copy across two systems without knowing
  which they are. What differs between systems reaches it as data from the driver that runs each
  one: how many of its computers the driver expects may run at once on this device, as Apple allows
  two macOS computers on a Mac, and whether a fork starts from memory or from the disk. That number
  is a guess, because other apps and faults of the device take places too. When the device refuses
  to start a computer for lack of a place, that refusal is a fact: the computer waits, and Silo shows
  which computers it knows hold the places." `sets` unchanged.
- **For the first build:** Does not block, and the first build has one system, so the property costs
  one value. It needs the refusal handled from the first day, because `restore-to-a-checkpoint`
  stops and starts main, and a leaked place would leave main down with no reason shown.

## `computer-image`: Where does a computer's system image come from?

- **Recommendation:** Split. Take the macOS half now, edited so that Silo keeps the installed
  system from before its first start. Leave the Linux half open: the first build makes no Linux
  computer, and the Linux image depends on `linux-vm-engine`.
- **Confidence:** high on the macOS half (Apple's documentation, read). The Linux half is not
  judged here.
- **What it decides, on one real case:** On macOS 27, Apple's framework can set up a new macOS
  computer on its first start: user name, password, automatic login and SSH on
  (`VZMacGuestProvisioningOptions`). It does that only on the first start after the install. If
  Silo keeps the system after main has started, a later fresh macOS computer is a copy of main's
  user, its password, its SSH host keys and its identity, and cannot be set up again. If Silo keeps
  it before the first start, each fresh macOS computer is set up on its own first start, and the
  copy was written by nothing that ran inside.
- **Options:**
  - Keep the installed system before its first start (proposed). Each fresh macOS computer pays its
    own first start of a few minutes. Tart's macOS 27 template does this with
    `--provisioning-opts=…enablesRemoteLogin=true` (verified, below).
  - Keep it after the first start (the answer leaves this open). A fresh computer is ready at once.
    It costs shared host keys and identity across computers, and a user Silo cannot change.
  - Packer typing keystrokes over VNC through Setup Assistant (Tart before macOS 27). Costs about 14
    minutes of timed waits per build on macOS 26, and breaks when Apple changes a screen.
- **Evidence:**
  - `VZMacGuestProvisioningOptions`, macOS 27.0: "requires guest macOS 27 or later […] macOS only
    evaluates these options on the first boot after restore […] can't use them to reconfigure macOS
    once the framework has already provisioned it." `enablesRemoteLogin` turns on SSH (verified:
    Apple's documentation JSON).
  - WWDC26 session 224 presents it: "When the guest boots for the first time, these parameters are
    automatically passed to Setup Assistant" (verified:
    https://developer.apple.com/videos/play/wwdc2026/224).
  - Tart's macOS 27 template uses those options, and still types keys for about 7 minutes to turn on
    Screen Sharing and turn off Gatekeeper, which the options do not cover. Its macOS 26 template
    types for about 14 minutes (verified: I summed the waits of `vanilla-golden-gate.pkr.hcl` and
    `vanilla-tahoe.pkr.hcl` in cirruslabs/macos-image-templates).
  - Apple's catalog of restore images (`mesu.apple.com/…/com_apple_macOSIPSW.xml`) lists only the
    latest build, 27.2.1 today, with a SHA-1 for each Mac model; its `VirtualMac2,1` entry points to
    the same file but has no SHA-1 (verified, downloaded). So "the restore image for the Mac's macOS
    version" may not be downloadable: Apple offers the latest one, and the framework's
    `VZMacOSRestoreImage.latestSupported` returns the latest the Mac supports (verified, Apple's
    documentation).
  - The download supports ranges: a request for the last 706 bytes returned 206 (verified, curl).
- **Risks and what would change this:** A Mac on macOS 26 or earlier cannot use the provisioning
  options, so this edit makes macOS 27 on the Mac the floor for a macOS computer. Another agent
  covers getting past Setup Assistant in depth; if it finds a way that works on 26, the floor can
  drop, and the rest of this edit still holds.
- **Proposed text** (the macOS half, as its own decision or as the whole answer with the Linux half
  left to `linux-vm-engine`): "A macOS computer starts from Apple's restore image: the latest one the
  Mac supports, as Apple's framework names it. Apple forbids shipping macOS installed, so each Mac
  downloads it from Apple, checks it against the checksum Apple publishes, and installs it. Silo
  keeps the installed system as it is before its first start, and deletes the restore image once the
  install has worked. Every macOS computer that is not a copy of another starts from a copy of that
  system and is set up on its first start: its user, and SSH with the key Silo made for it. A
  download that fails resumes where it stopped. No computer starts from an image that is incomplete
  or does not match its checksum. With no connection, Silo says what it has to download and how
  large it is."
- **For the first build:** Blocks `computers-main-on-a-mac`. The builder needs: the floor (macOS 27
  on the Mac, or the other agent's answer), the kept system taken before the first start, and the
  first start's settings, which go beyond Apple's options: passwordless `sudo` for the agents, sleep
  off, and how Claude Code and Codex keep their logins over SSH (cross-cutting finding 3).

## `first-run-time-macos`: How long does the first run take with a macOS main?

- **Recommendation:** Take with an edit. "Measured, not chosen" is right. The size it states is off
  by more than half, and the answer leaves out two things that make a first run fail halfway: free
  disk and a Mac going to sleep.
- **Confidence:** high on the download (measured size, simple arithmetic). The install time is not
  measured anywhere I found.
- **What it decides, on one real case:** You say yes to main on a MacBook at home on 100 Mbit/s.
  The download alone takes 34 minutes. You close the lid after 20 minutes. Without a power
  assertion the Mac sleeps and the download stops; with the lid closed on battery it sleeps anyway.
  On a 256 GB Mac with 30 GB free, the download fills the disk before the install starts.
- **Options:**
  - One wait, shown step by step (the answer). Costs the wait, once per Mac.
  - Agents start in a Linux computer meanwhile. Costs two places, a move, and the first build has
    no Linux.
- **Evidence:**
  - 25,595,954,706 bytes for macOS 27.2.1 (verified). At 50, 100, 300 and 1,000 Mbit/s that is 68,
    34, 11 and 3.4 minutes (arithmetic). From this container Apple's CDN served 200 MB at 28 MB/s
    (verified, one run, a datacenter link).
  - Install time: a guide puts `tart create --from-ipsw latest` at about 20 minutes with a 15 GB
    download (not verified, anecdote: keepcoding.io). Tart's Packer builder adds
    `create_grace_time = "30s"` because the framework's install keeps running after it reports done,
    and its macOS 27 template waits 120 s for first-start provisioning (verified, templates above).
  - A background process on macOS gets throttled. One developer measured the framework writing at
    30 to 40 MB/s from a background runner, and 831 MB/s once App Nap was off and a `ProcessInfo`
    activity with `.idleSystemSleepDisabled` held (verified, read:
    https://developer.apple.com/forums/thread/828750). The same applies to a long download and
    install run by the engine.
- **Risks and what would change this:** Once measured on polarzero's Mac, the answer gets the
  number. If the install alone is over 30 minutes, the Linux-first option is worth a second look.
- **Proposed text:** "It is measured, not chosen: the download of Apple's restore image, about
  26 GB for macOS 27, its install, and the first start, on polarzero's Mac. Before it starts, Silo
  checks that the Mac has the free disk the download and the install need, and says how much is
  missing if not. Silo starts the download as soon as you say yes to main, shows each step and the
  time left, keeps the Mac awake while it runs, and keeps going if you close the window. A download
  that stops resumes where it stopped. Silo keeps the installed system, so later macOS computers
  start from a copy and skip the download and the install. Until it is measured, Silo states no
  time in public."
- **For the first build:** Blocks `computers-main-on-a-mac` only formally. The builder needs the
  disk check, the power assertion (`ProcessInfo.beginActivity` with `.idleSystemSleepDisabled`, or
  an IOKit assertion), and App Nap off for the engine.

## `files-through-a-channel`: How do files go in or out of a computer?

- **Recommendation:** Take with an edit. The rule is right. As worded, "only through a channel you
  grant" is checked by a hostile test that fails on the first day: in the first build the network
  is open and the way in over SSH lets any tool on your Mac read main's files, and you granted
  neither for files. Say which channels carry files, and make the check one that can pass.
- **Confidence:** medium. The facts are verified; that the property fails is my reading of the
  words `channel` ("One way through the boundary") and `granting`.
- **What it decides, on one real case:** An agent with `sudo` in main wants a file on your Mac. It
  cannot write there through a shared folder, because none exists. It can push it to a server and
  wait for you to download it, or leave it in `~/work/app` where your editor, over SSH, opens it.
  Both are channels. The first is the network, which nobody granted file by file; the second is
  the way in, which you granted once, for every computer (`reaching-a-computer`).
- **Options for how a file goes in and out:**
  - A copy over the SSH way in (proposed for the first build). Silo copies the file with `scp` or
    `sftp` as a client, into a folder of main or out to a folder Silo picks on the Mac. Nothing is
    added to the virtual machine. Lima's `limactl copy` works this way (not verified here).
  - A directory share swapped in only during an import. `VZVirtioFileSystemDevice.share` can be set
    while the machine runs (verified: its declaration is `var share: VZDirectoryShare? { get set }`),
    and macOS guests mount it by themselves with `macOSGuestAutomountTag` (verified). It costs a
    device present from the first start, and a host directory reachable while it is set. Tart's
    `--dir` and UTM use shares, always on.
  - A folder always visible on the Mac, as OrbStack does. Costs what the answer says.
- **Evidence:** Apple's documentation of `VZVirtioFileSystemDevice` (verified): it reads and writes
  with the user's UID and ignores the guest's changes of owner. Files from a hostile computer that
  reach the Mac should carry `com.apple.quarantine`, as a browser download does, so Gatekeeper
  checks an app before it opens (known macOS behavior, not verified here).
- **Risks and what would change this:** If polarzero reads "a channel you grant" to include the
  network and the way in as granted when main is made, the answer is fine as it is, but then the
  hostile test needs to say what it attacks.
- **Proposed text:** answer: "Files go in or out of a computer only through a channel. The channels
  that carry files are the ones Silo lists for that computer: the network, the way in from your Mac,
  and each import or export you start. No folder is shared between a computer and your Mac. A copy
  you start is made once and leaves nothing shared." `sets`:
  `{"in":"computers/properties","id":"files-cross-only-by-channel","says":"Files go in or out of a computer only through a channel it lists.","kind":"safety","check":["hostile test"]}`.
  The hostile test then has a target: an agent with `sudo` in main moves a file to the Mac by any
  means not on the list.
- **For the first build:** Blocks `files-imported-from-your-mac` and `computers-main-on-a-mac`. The
  builder needs: copies over SSH, no directory share in main's configuration (it also keeps the
  configuration fixed for a saved state later), files brought out land in one folder with the
  quarantine flag.

## `pause-every-computer`: Can you pause every running computer at once?

- **Recommendation:** Take with an edit. Resuming only what the action paused is right. It does not
  say what happens when the Mac restarts between the pause and the resume, which for a macOS
  computer turns a pause into a stop.
- **Confidence:** medium. That a paused virtual machine ends with its process is Apple's stated
  behavior. What the framework does on a Mac restart while paused is not measured.
- **What it decides, on one real case:** At 23:00 you pause everything. Main is paused. Overnight
  macOS installs an update and restarts the Mac. In the morning main is not paused: the paused
  virtual machine lived only in the memory of the process that held it. If the engine reads
  "wanted: running, paused by the pause-all" and starts it, an agent you froze is running again
  before you looked. If it reads "stopped", your one-click resume should start it cold and say so.
- **Options:**
  - Keep the pause in memory; after a restart the computer is stopped, and a resume starts it from
    its disk and says what was lost (proposed for now). Costs the memory state.
  - Save the paused state to disk (`saveMachineStateTo`, macOS 14+), as `tart suspend` does, so it
    survives the restart. Costs a file of up to the computer's memory, a few seconds per GB, a
    configuration without the devices that cannot be saved, and a file that only this Mac and user
    can read (verified, below). Belongs with `checkpoints-with-memory`.
- **Evidence:**
  - "If the app terminates unexpectedly, the system should clean up its resources, and that includes
    killing any VMs that it might've left running" (verified, Apple DTS:
    https://developer.apple.com/forums/thread/848542).
  - Save: "fails if the VM isn't in a paused state"; restore "places it in the paused state"; a
    configuration may not be savable (`validateSaveRestoreSupport`) (verified, Apple's
    documentation). Tart's `--suspendable` "disables or replaces devices that do not support VM
    suspension, such as audio, entropy and some input devices" (verified, Tart source). A saved
    state of about 1.25 GB took 1.5 s to save and 3.4 s to restore, once App Nap was off (verified,
    thread 828750). "No other Mac or user account can read another's save file, or restore the
    virtual machine" (verified, WWDC23 session 10007).
  - A paused computer still holds its memory and, I expect, one of Apple's two places (not
    verified).
  - The notes' case has "main and two forks". With main on macOS, its forks are macOS, and only one
    runs beside main. The same impossible case is in `computers-stopped-and-paused` and
    `engine-matches-wanted`. It should be main, one fork and a Linux computer.
  - A pause of more than a minute or so breaks connections that were open, such as an agent's
    streamed reply from the model. The disk loses nothing; the agent sees an error. Not measured.
- **Proposed text:** "One action pauses every running computer. One action resumes the computers
  that action paused. A computer that was already paused, by you, a rule or a precaution, stays
  paused until you resume that one. A computer that fails to pause is named in an alert, and the
  others stay paused. A paused computer that stops before it is resumed, because the Mac restarted
  or the program holding it ended, comes back stopped and never running. Resuming it then starts it
  from its disk, and Silo says that what was running is lost."
- **For the first build:** Does not block much: with only main, pausing all is pausing main. The
  builder needs the restart case, since `engine-matches-wanted` is in the same build.

## `what-stop-ends`: Which processes does stopping a computer end?

- **Recommendation:** Take with an edit. The answer is sound and matches what Tart does. "How a Linux
  computer is asked to shut down is to find out" should cover macOS too: Apple does not document
  what `requestStop()` does to a macOS guest, and a reported bug keeps a place taken after a stop.
- **Confidence:** medium. Tart's code is read; how the framework asks the guest is not documented
  anywhere I found.
- **What it decides, on one real case:** You restore main. Silo stops it first: the network is cut,
  `requestStop()` is sent, and after 30 s the virtual machine is ended. If the macOS guest ignores
  the request, every stop of main is a power cut after 30 s, and every restore waits those 30 s.
  Then Silo starts main from the restored disk and, on a Mac hit by the bug above, the start fails
  with "limit exceeded" until the Mac restarts.
- **Options:**
  - Ask, wait 30 s, then end it (the answer). Tart's `tart stop` has the same default of 30 s, then
    `SIGKILL` (verified:
    https://github.com/cirruslabs/tart/blob/main/Sources/tart/Commands/Stop.swift).
  - End at once with `stop()`, "a bit like pulling out the power cable" (Apple DTS, below).
  - Ask the guest over SSH with `shutdown -h now`. Costs trust in a server an agent with `sudo`
    controls; the 30 s fallback still covers it.
- **Evidence:**
  - `requestStop()` "asks the guest operating system to stop running", and its return value "doesn't
    reflect whether the VM will exit or has exited" (verified, Apple's documentation). A developer
    notes the framework seems to have no ACPI power button (not verified:
    https://developer.apple.com/forums/thread/702160).
  - When the disk image is free to reuse: "Things should work as I expected on macOS 27 and later.
    On earlier systems it's possible for you to receive the notification before the guest is fully
    done with the disk image", with `flock` on the image as the workaround (verified, Apple DTS:
    https://developer.apple.com/forums/thread/849007). This decides when the checkpoint of
    `restore-saves-first` can be taken after a stop.
  - The place not given back after a guest shutdown or `tart stop`, on macOS 26.5 (verified as
    reported, not reproduced: thread 830119).
- **Proposed text:** "Stopping or dropping a computer ends every process in it, detached ones
  included. Once a stop begins, nothing crosses the computer's boundary. Silo asks the system inside
  to shut down, and after 30 seconds ends the virtual machine itself, so a program that ignores the
  request, or an agent that holds it up, still ends. The disk keeps what the system inside had
  written to it before the end. How each system is asked to shut down, and how often it is done
  within the 30 seconds, is to find out on macOS and on Linux." `sets` unchanged.
- **For the first build:** Needed by `restore-to-a-checkpoint`, so it blocks. The builder needs:
  on a Mac before macOS 27, wait on `flock` before touching the disk; after every stop, check the
  next start for the limit error and report it (`neutral-core` above). Mac test: 20 stops of a busy
  macOS 27 guest, counting how many shut down within 30 s and whether any place leaks.

## Cross-cutting findings

1. **The first build should require macOS 27 on the Mac.** It brings
   `VZMacGuestProvisioningOptions` (a user and SSH with nobody clicking), the fix of disk release
   after a stop (Apple DTS, thread 849007), and DiskImageKit with ASIF overlay layers "for
   copy-on-write semantics for snapshots" (WWDC26 session 224, verified), which bears on
   `checkpoint-per-system` and the XFS stalls of PR #347. polarzero's Mac is the first and only Mac
   of the first build (`who-it-is-for`). No entry of the plan states a minimum macOS. One should,
   with these three reasons.
2. **The two places are shared and can leak.** Other apps (UTM, Tart, Parallels with macOS guests)
   take the same two places, and macOS 26.5 is reported to leak one after a stop. Every entry that
   counts places (`task-macos-limit`, `neutral-core`, `computers-per-mac`) should treat the count as
   a guess and the device's refusal as the fact. Three notes use a case Apple's limit rules out:
   "main and two forks" running together (`pause-every-computer`, `computers-stopped-and-paused`,
   `engine-matches-wanted`).
3. **The agents' logins over SSH, a risk not in the plan.** The first build is `ssh main`, then
   `claude`. Claude Code keeps its login in the macOS keychain, and many users report that over
   SSH it cannot read it and asks for `/login` again, with `errSecInteractionNotAllowed`; the usual
   workaround is `security unlock-keychain` in the SSH session, which others say does not help
   (not verified: https://github.com/anthropics/claude-code/issues/29816,
   https://oldeucryptoboi.substack.com/p/using-claude-code-cli-over-ssh-fixing). Codex can be told
   to keep its login in a file with `cli_auth_credentials_store = "file"` (not verified:
   https://developers.openai.com/codex/auth). It also bears on
   `agent-programs-logins-kept-out-of-copies`, which assumes the login is a keychain item. Mac test:
   in a fresh macOS 27 computer with automatic login, log in to Claude Code over SSH, disconnect,
   reconnect, and see whether it still knows you, before and after a restore.
4. **Apple's license and `where-it-runs`.** Its "cloud of its own" cannot hand out macOS mains as a
   shared service: section 2B excludes "service bureau, time-sharing", and section 3 allows leasing
   only a whole Mac to one user for at least 24 hours, for "Permitted Developer Services" (CI,
   building, testing). A main for a non-developer, rented, is outside both (verified, the Tahoe
   PDF). The challenge: `where-it-runs` should say that macOS computers in a cloud are limited to
   whole Macs leased under Apple's terms, or that the cloud is for Linux computers.
5. **`idle-computer-gives-memory-back` (taken) may not hold for main.** Apple's framework has only a
   traditional balloon, and whether a macOS guest answers it is unknown; one forum post says macOS
   has no balloon driver (not verified). `memory-given-back` already says this goes to polarzero
   if it fails. The challenge: in the first build main is the only computer, so this property is
   the first thing polarzero's Mac would show as false. If a macOS guest gives nothing back, the
   property should say "A Linux computer that is idle gives memory back", and main's memory should
   be a size you choose.
6. **The engine holds main's life.** A virtual machine ends with the process that holds it (Apple
   DTS, thread 848542). If the engine holds main, every crash of the engine is a power cut of main.
   `engine-outlived-by-computers` (one helper process per computer, as Tart runs one process per
   virtual machine) should be in the first build, or the first build should say that an engine crash
   ends main.
7. **What the first build cannot run.** No Docker in main, and no Linux computer to run it in
   until the third build. Projects whose tests need a database in Docker cannot be tested in Silo
   during the first two builds.

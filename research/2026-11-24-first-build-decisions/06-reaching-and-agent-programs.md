# Reaching a computer and running agent programs: a second opinion on five decisions

An agent's review, 24 November 2026, for polarzero. It changes nothing under `plan/`. Each
section gives a recommendation, the evidence behind it, and, where an edit is proposed, the exact
text. "Verified" means I read the source code or the vendor's page myself, or ran the test. "Not
verified" means the claim rests on a search summary, a forum post or my own knowledge.

| id | Recommendation | Confidence | One-line reason |
|---|---|---|---|
| `computer-reached-over-ssh` | Take with an edit | high on SSH itself, medium on the edit | SSH is right, and every tool that matters uses OpenSSH. But two of its promises are false as written: a port forward or key you set for every host still applies to Silo's entries (tested), and a plain terminal is not free of links back (OSC 52 clipboard). The answer also does not say how ssh reaches the computer, and that choice decides what Silo can see |
| `reaching-a-computer` | Take with an edit | medium | One file of named entries, included once, is what Lima and OrbStack do. But the `Include` line must be the first line of `~/.ssh/config` (tested), "no prompt" is false for the Codex app and Claude desktop, and in the first build main's network is open, which "nothing else is open" does not allow for |
| `documented-surfaces` | Take with an edit | high | Right rule. It needs one sentence: a documented path is not a documented format, and source code is not documentation. Without that, the hook field `transcript_path` makes the rule unclear on its first real case |
| `unmodified-agent-programs` | Take as written | high | Both vendors document installs that run unchanged in a macOS computer reached over SSH. Two facts belong in its notes: a restore brings back a stale login, and on macOS Claude Code's login may sit in the Keychain, which Silo cannot leave out of a fork |
| `silo-is-light` | Take with an edit | medium | The principle is sound and polarzero asked for it. "As little as it can" has no scale; tie each target to what the computer itself costs. An idle macOS computer holds 3 to 5 GB of memory and about 54 GB of disk |

## `computer-reached-over-ssh`: Which way in reaches a computer?

- **Recommendation:** Take with an edit. SSH is the only way in that every editor and agent app
  supports, but the answer promises two things that do not hold. It also leaves out how ssh
  reaches the computer, which decides whether Silo can see who opens the way in.
- **Confidence:** high that SSH is right. It rests on the vendors' own pages: Claude desktop, the
  Codex app, Zed, VS Code and the JetBrains Toolbox App all run OpenSSH. Medium on the transport I
  propose, which is not yet tried against a macOS guest.

### What it decides, on one real case

You run `ssh main` in Ghostty and, in another window, VS Code opens `~/work/app` in main over
Remote-SSH. Overnight Claude Code runs in main with every check off. Three things can then reach
your Mac:

1. VS Code's server in main. A remote extension can open a terminal on your Mac and type into it
   (`workbench.action.terminal.newLocal` and `sendSequence`).
2. Ghostty itself, if anything in main prints an OSC 52 sequence. Ghostty writes your Mac's
   clipboard with no prompt by default, and you later paste into a terminal on your Mac.
3. A `RemoteForward` or extra `IdentityFile` you once set under `Host *`. It applies to `main` too,
   though Silo's entry comes first.

The answer covers the first. It does not cover the second or the third.

### Options

| Option | What it costs | Who does it |
|---|---|---|
| SSH with your own tools (current answer) | sshd in each computer, which an agent with sudo can stop. The links back that some tools open | Lima, OrbStack, Podman machine, Tart, Codespaces |
| Silo's own shell and editor only | Every SSH-based tool: Claude desktop, the Codex app, VS Code, Zed, T3 Code | Nobody among the products compared |
| SSH, with Silo choosing the transport (proposed) | One small program of Silo's on the Mac (a `ProxyCommand`), and tools that only speak to a `host:port` | systemd's `systemd-ssh-proxy`, OrbStack (`ssh orb`), Lima's vsock forwarder |

The transport is the real choice left open. There are four ways for ssh on the Mac to reach sshd
in a macOS computer:

| Transport | Fits `network-stack`? | Listens on the Mac? | Silo sees who connects? | Needs a program of Silo's inside? |
|---|---|---|---|---|
| The computer's IP on Apple's NAT (Tart: `ssh admin@$(tart ip vm)`) | No: `network-stack` says never Apple's NAT | No | No | No |
| A `localhost` TCP port forwarded by the stack (Lima's user network, Podman's gvproxy `-ssh-port`, OrbStack's port 32222) | Yes | Yes: any process of any user on the Mac can connect | Only by matching ports to processes, which can race | No |
| A Unix socket on the Mac that the stack forwards to port 22 inside, with `ProxyCommand` and `ProxyUseFdpass` | Yes | No TCP port. The socket has mode 0600, so only your user's processes can connect | Yes: `LOCAL_PEERPID` on the socket gives the process, and its parents name the app | No |
| vsock: the host connects to a port in the guest | Goes around the stack | No | Yes | Yes: macOS sshd does not listen on vsock. Tart's guest agent shows a macOS guest can accept host-made vsock connections |

I recommend the Unix socket. No TCP port is open on your Mac. Nothing of Silo's runs inside the
computer. Each SSH connection passes the engine, so "the way in is open, opened by VS Code" is a
fact read on the Mac, from outside the computer. That is what `facts-before-reports` asks for,
and it is how Silo can "show such a link among the computer's channels while it is open". Without
it, the first build has no way to know a link is open, since the probe inside macOS is held back
(`macos-ships-facts-first`). With fd passing, the helper exits once it has handed ssh the
connected socket. Nothing of Silo's stays between ssh and the computer for the length of the
session, which suits `silo-is-light`.

One consequence to accept knowingly: the way in then rides on Silo's network stack. If the stack
stops, the computer has no network and no way in either. vsock would keep the way in when the
network is down, but it needs a program of Silo's in the computer.

### Evidence

- **Verified (vendor page).** Claude desktop: "Desktop connects by running the OpenSSH client on
  your computer, so it uses your `~/.ssh/config`". It honours `ProxyJump` and `ProxyCommand`, and
  "Desktop leaves `ForwardAgent`, `ForwardX11`, `LocalForward`, `RemoteForward`,
  `DynamicForward`, and `Tunnel` off, even where your SSH configuration turns them on". It
  installs "a helper program and Claude Code under `~/.claude/remote/`", about 1 GB per user, and
  needs `Subsystem sftp`. In SSH sessions "the desktop app delivers each user's connected claude.ai
  connectors to Claude Code directly", and "Desktop renews that sign-in for the task only while
  Desktop is running, connected, and signed in".
  https://code.claude.com/docs/en/desktop
  So Claude desktop forwards nothing over SSH, but it does carry your Claude sign-in and your
  claude.ai connectors (mail, drive, and others) to the agent in the computer. That is a channel
  to the internet with your accounts, not to your Mac's files.
- **Verified (vendor page).** The Codex app "reads concrete host aliases from `~/.ssh/config`,
  resolves them with OpenSSH, and ignores pattern-only hosts". It starts the Codex app server over
  SSH in the remote user's login shell, so `codex` must be on that PATH, and you "install and
  authenticate Codex on the remote host". https://learn.chatgpt.com/docs/remote-connections.md
- **Verified (vendor page).** Zed "shells out to the `ssh` on your path", uses one ControlMaster
  per project, and installs `~/.zed_server/zed-remote-server-*` on the remote. macOS 12 and later
  is a supported remote. https://zed.dev/docs/remote-development
- **Verified (vendor page).** VS Code Remote-SSH installs VS Code Server on the remote, needs 1 GB
  of memory there and recommends 2 GB, and supports macOS hosts with Remote Login enabled.
  https://code.visualstudio.com/docs/remote/ssh. The Marketplace page's security note reads: "A
  compromised remote could use the VS Code Remote connection to execute code on your local
  machine." https://marketplace.visualstudio.com/items?itemName=ms-vscode-remote.remote-ssh
- **Verified (write-up).** Calif, 18 August 2025: a remote extension opens a terminal on the
  laptop with `workbench.action.terminal.newLocal` and types into it with `sendSequence`. It
  works "in the default settings of Cursor or VS Code", and Microsoft indicated it will not change
  the extension. https://blog.calif.io/p/vibe-hacking-abusing-developer-trust
- **Verified (vendor FAQ).** JetBrains: "If you use the JetBrains Gateway connection, only Linux
  servers are supported as suitable for the backend"; the Toolbox App supports macOS servers and
  uses "the integrated OpenSSH". So Gateway cannot open main at all.
  https://www.jetbrains.com/help/idea/faq-about-remote-development.html
- **Not verified (forum).** Cursor's own SSH extension forces askpass and broke a `ProxyCommand`
  that ran `sshpass`. A plain `ProxyCommand` like Silo's would not prompt, so it should not be
  affected. There are also open reports of agent tools hanging from Mac to Mac.
  https://forum.cursor.com/t/anysphere-ssh-client-doesnt-work-with-sshpass/121483,
  https://forum.cursor.com/t/remote-ssh-mac-mac-agent-tools-hang-indefinitely-often-on-git-integrated-terminal-git-is-fine/169295
- **Verified (test, OpenSSH 9.6p1 here).** Config files (tested, scratch script, not in the repo):
  `Include silo.conf` on the first line, then the user's `Host *` with `ForwardAgent yes`,
  `RemoteForward 9999 localhost:22` and `IdentityFile ~/.ssh/id_ed25519`. Then
  `ssh -G main` gives:
  `forwardagent no` (Silo's value wins), `remoteforward 9999 [localhost]:22` (the user's forward
  still applies), and both identity files, Silo's and `~/.ssh/id_ed25519`. ssh_config(5) says
  why: the first value wins "unless noted otherwise", and `IdentityFile`, `LocalForward` and
  `RemoteForward` add up. https://man.openbsd.org/ssh_config. With `-A` on the command line,
  `forwardagent yes` wins over the file, and Zed lets you pass `-A`.
- **Verified (test).** `ClearAllForwardings yes` in Silo's entry also drops a `-L` given on the
  command line. That would break the editors and logins that need one: T3 Code forwards its
  server's port back to the desktop, and Codex's documented login on a remote machine is
  `ssh -L 1455:localhost:1455`. So Silo cannot simply clear forwardings in its entry.
- **Verified (vendor page).** Ghostty: "The default is to allow clipboard reading after prompting
  the user and allow writing unconditionally" (OSC 52). https://ghostty.org/docs/config/reference
  Silo cannot see this link: it travels inside the encrypted SSH stream.
- **Verified (source).** Lima turns on sshd in a macOS guest with
  `launchctl load -w /System/Library/LaunchDaemons/ssh.plist`, run as root by a LaunchDaemon it
  writes onto the disk before the first start (`pkg/guestagent/fakecloudinit/fakecloudinit_darwin.go`,
  `pkg/guestpatch/macos/macos_darwin.go`, commit `683f731`). Lima uses SSH over vsock only for
  Linux guests by default (`useSSHOverVsock := *inst.Config.OS == limatype.LINUX`,
  `pkg/driver/vz/vm_darwin.go`). For macOS guests it reaches sshd through its user-space network.
- **Verified (source and PR).** Lima's vsock forwarder listens on `localhost` and bridges to vsock
  port 22. Its author declined a direct connection because "VM's VSOCK should not be open to the
  host unless the process that is starting the VM is mediated". Measured in that PR: 3.57 Gbit/s
  over vsock against 2.34 Gbit/s through the user-space network.
  https://github.com/lima-vm/lima/pull/3979
- **Verified (source).** Tart's guest agent listens on `AF_VSOCK` port 8080 inside macOS guests
  (`tart-guest-agent/internal/command/root.go`). Tart's `ControlSocket.swift` dials it with
  `VZVirtioSocketDevice.connect(toPort:)`. Apple's vsock(4) still says "only stream connections
  initiated from a guest are supported", so that man page is out of date for this case.
  https://keith.github.io/xcode-man-pages/vsock.4.html
- **Verified (source).** gvisor-tap-vsock can listen on a Unix socket on the host and dial a TCP
  port of the guest through its own stack (`pkg/services/forwarder/ports.go`, "unix-to-tcp
  proxy", commit `197eea0`). The Unix socket transport above needs nothing new from it.
- **Verified (vendor page).** OrbStack forwards your SSH agent into its Linux machines by default.
  Claude desktop forces agent forwarding off. https://docs.orbstack.dev/machines/ssh
- **Partly verified.** A forwarded agent gives more than signatures. CVE-2023-38408: remote code
  execution in ssh-agent "if an agent is forwarded to an attacker-controlled system", fixed in
  OpenSSH 9.3p2. https://nvd.nist.gov/vuln/detail/CVE-2023-38408,
  https://blog.qualys.com/vulnerabilities-threat-research/2023/07/19/cve-2023-38408-remote-code-execution-in-opensshs-forwarded-ssh-agent
  So agent forwarding off is right. The cost is that `git push` from main needs a credential that
  lives in main.
- **Not verified.** `LOCAL_PEERPID` on a macOS Unix socket returns the peer's pid. It is in
  `<sys/un.h>` and is how macOS servers identify a local caller. Not tried here: no Mac.

### Risks and what would change this

- If the Codex app or Claude desktop rejects a `ProxyCommand` entry with `ProxyUseFdpass` on a Mac,
  fall back to a plain `ProxyCommand` that relays bytes and stays for the length of the session.
  If they reject `ProxyCommand` altogether, use a `localhost` port, shown as a channel. Test: add
  the entry, connect from each app, run `claude` and `codex` inside.
- If macOS sshd cannot be turned on before the first start without the kernel mounting the disk,
  the first-run work (`computer-image`) changes, not this decision. Lima mounts the fresh disk,
  which nothing inside has written yet.
- "Restoring a checkpoint from before brings it back" holds only if the agent's change sits on
  the disk. An agent that changes only the running sshd is undone by a restart.

### Proposed text

> A computer is reached over SSH, so the terminal, the editor and the agent apps people already
> use keep working. You work in a computer yourself the same way: a terminal over SSH, your
> editor opened on a folder of the computer, or a tool such as T3 Code connecting in. A macOS
> computer's screen opens in Silo's app. The SSH server is the computer's own, with a key Silo
> made, kept on your Mac. ssh on your Mac reaches it through Silo: the SSH entry Silo writes hands
> ssh a connection Silo opens to the computer, so no port of your Mac listens for it, and Silo
> sees each connection and which program on your Mac opened it. An agent with sudo can stop the
> server or change it. Silo then shows on that computer that the way in does not answer or that
> its host key changed, and restoring a checkpoint from before brings it back. Silo's entry turns
> off agent forwarding and forwards no port. Your own SSH settings can still add a port forward
> or a key to every host, Silo's entries included, so Silo reads what ssh will use for each entry
> and shows any forwarding there. An editor that runs a server inside the computer, as VS Code
> does, keeps a link back to your Mac, and VS Code's documentation says a compromised remote could
> use it to run code on your Mac. Silo shows such a link among the computer's channels while it is
> open. A terminal can let a program in the computer write your Mac's clipboard, which some
> terminals allow by default and Silo cannot see. What each tool lets a computer reach on your
> Mac is to find out, one tool at a time.

### For the first build

It blocks `agent-programs-reach-every-computer`, which the first build needs. The builder needs
the transport fixed: a Unix socket per computer and a `silo` helper used as `ProxyCommand` with
`ProxyUseFdpass yes`. Also `HostKeyAlias <name>`, a known-hosts file of Silo's with
`StrictHostKeyChecking yes`, `IdentitiesOnly yes` and `ForwardAgent no`. The screen sentence can
stay. The first build has no app, so main's screen is simply not there yet.

## `reaching-a-computer`: How does any program on the Mac reach any computer?

- **Recommendation:** Take with an edit. The design is what Lima and OrbStack do. Three facts in
  the answer are wrong or missing.
- **Confidence:** medium. The `Include` facts are tested. Whether the Codex app follows `Include`
  is not known.

### What it decides, on one real case

You install Silo. It asks once to add a line to `~/.ssh/config`, and you say yes. Silo makes main
and writes `Host main` into `~/.silo/ssh/config`. You type `ssh main`, and Claude desktop
connects when you type `main` as its SSH host. In the Codex app, `main` shows up under Settings >
Connections, and you still enable it there.

### Options

| Option | What it costs | Who does it |
|---|---|---|
| One included file, one named entry per computer (current answer) | One edit of your `~/.ssh/config`, and a standing grant | OrbStack (adds `orb` for you), Lima (you add `Include ~/.lima/*/ssh.config`), Codespaces (`gh codespace ssh --config`) |
| A pattern entry (`Host *.silo`) with one `ProxyCommand` that resolves any name | No file to rewrite per computer | Teleport (`tsh config`), Tailscale SSH. The Codex app ignores pattern-only hosts, so it would never list a computer |
| Silo writes into `~/.ssh/config` itself | Silo edits a file you own on every change | Few, and people dislike it |

The pattern entry is ruled out by the Codex app (verified above). The current answer stands.

### Evidence

- **Verified (test).** If the `Include` line comes after the user's `Host *`, `ForwardAgent yes`
  wins for `main`. If it sits inside one of the user's `Host` blocks, Silo's entries apply only
  when that block matches: `ssh -G main` printed `user root` and none of Silo's settings.
  ssh_config(5) says the same: "Include directive may appear inside a Match or Host block to
  perform conditional inclusion". So the "first" in "Silo's entries come first" is a property of
  one line's position, and it can be checked: Silo runs `ssh -G <name>` after writing and compares.
- **Verified (vendor pages).** "Every tool then reaches every computer by name, with no prompt" is
  wrong for two apps. The Codex app needs each host added or enabled under Settings > Connections.
  Claude desktop asks "Connect to `<host>` for the first time?" on the first connection. Not
  verified: whether a known-hosts file of Silo's that already holds the key skips Claude desktop's
  question.
- **Not verified (forum).** The Codex app crashed at launch on a large SSH config: discovery ran
  many `ssh -G` at once and failed with `spawn EBADF`. The reporter said it was later fixed.
  https://community.openai.com/t/codex-desktop-app-fails-to-launch-when-large-numbers-of-ssh-host-entries-present/1379977
  With forks made by agents, the file grows. Silo should remove entries of computers in the bin.
- **Not verified.** Whether the Codex app's list of hosts follows `Include`. It resolves each alias
  with OpenSSH, which does follow `Include`, but the list itself comes from the app's own reading
  of the file. This is a test on a Mac, and the most important one for this entry. If it fails,
  Silo must write a `Host` line per computer into `~/.ssh/config` itself, or the Codex app never
  lists a computer.
- **From the plan.** The first build has no rules and no closed network (first-build proposal,
  "What is left out"). Main can reach the internet from its first start. "Nothing else is open in
  a new computer until you, or a rule you set, open it" is then false on day one, unless making
  main asks you to open its network.

### Risks and what would change this

- If the Codex app does not follow `Include`, add the fallback above to the answer.
- If polarzero does not want Silo to edit `~/.ssh/config` at all, the Lima way remains: Silo
  prints the line and you paste it yourself.

### Proposed text

> Silo keeps one file of SSH entries: one named entry per computer, with its key. With your yes,
> Silo adds one line that includes that file, as the first line of your Mac's SSH settings, and
> checks that ssh applies its entries. That yes grants your Mac the way in to every computer,
> present and future, and Silo shows it among each computer's channels. Every tool that reads your
> SSH settings then reaches every computer by name. An app that asks before it uses a new host,
> as the Codex app does, still asks. Nothing else is open in a new computer until you, or a rule
> you set, open it. A repository host is a host like any other.

### For the first build

It blocks it, through `agent-programs-reach-every-computer`. The first build has only main, so
"including one made a moment ago" is not exercised yet. The builder needs: the `Include` line
first, a check with `ssh -G`, and one concrete `Host` per computer. Separately, the first build
needs a sentence on main's network: open when main is made, with your yes, and shown as a channel.
That belongs to the first build's own feature, not this entry.

## `documented-surfaces`: How does Silo integrate with an agent program?

- **Recommendation:** Take with an edit. One sentence settles the two ambiguous cases the first
  build meets at once.
- **Confidence:** high. It rests on the vendors' terms, already read in `logins`, and on how often
  both programs ship. Claude Code's native install updates itself in the background on the
  `latest` channel by default.

### What it decides, on one real case

Claude Code's hooks hand Silo `transcript_path`, for example
`/Users/you/.claude/projects/.../transcript.jsonl`. The path is documented. The format is not, and
the docs say the file "is written asynchronously and may lag the in-memory conversation". Under
the answer as written, Silo may read "the files its documentation names", but must not "rely on
behaviour the vendor does not document". So it may open this file, but it may not rely on the
JSON lines inside. The answer should say which wins.

Codex is the second case. Its source code is public, so every format is visible in the code. Is
reading `~/.codex/sessions` as the code writes it "documented"? It should not count. The code
changes with every release without notice. What Codex does document for other programs is the
app server's JSON-RPC protocol, with `codex app-server generate-json-schema`, one schema per
version.

### Options

| Option | What it costs | Who does it |
|---|---|---|
| Documented surfaces only (current answer) | Features where vendors document nothing | Most products built on Claude Code (Claude Agent SDK clients) and on Codex (app-server clients such as T3 Code) |
| Whatever works, with tests | Breakage after updates, and the terms (Anthropic's Commercial Terms forbid reverse engineering) | Tools that read `~/.claude/projects` transcripts to show history |

### Evidence

- **Verified (vendor page).** The hook input fields include `session_id`, `transcript_path` and
  `cwd`. https://code.claude.com/docs/en/hooks
- **Verified (vendor page).** The Codex app server documents a JSON-RPC protocol over stdio, a
  WebSocket or a Unix socket, and `generate-json-schema` "for the Codex version you ran".
  https://developers.openai.com/codex/app-server
- **Verified (vendor page).** Claude Code's login sits "in the encrypted macOS Keychain", and the
  name of the Keychain item is not documented. When the Keychain is locked, as in an SSH session,
  the login goes to `~/.claude/.credentials.json`, which is documented.
  https://code.claude.com/docs/en/authentication. Under this rule, Silo can leave the file out of
  a fork (`fork-and-sign-in`) but not a Keychain item. See the cross-cutting findings.

### Risks and what would change this

If a feature polarzero calls essential can only be built on a format no vendor documents, this
rule says Silo does without it. That is the intended cost. Ask the vendor first.

### Proposed text

> Silo works with an agent program only through what its vendor documents for others to use: its
> command-line options, settings, hooks, environment variables, the protocols it documents for
> other programs, and the files its documentation names. A file whose path is documented and whose
> contents are not is opened only as a whole: Silo may copy it, keep it out of a copy, or show
> that it changed, and does not read meaning from its contents. A program's source code is not
> its documentation. Silo does not patch a program, read its private files, or rely on behaviour
> the vendor does not document. Where the documentation says nothing, Silo does without, and the
> feature that would need it says so for that program.

### For the first build

It blocks it only through `agent-programs-run-unmodified`. The first build reads nothing from the
programs. It needs this decision only so that nothing in it starts parsing transcripts.

## `unmodified-agent-programs`: Which agent programs run in a computer?

- **Recommendation:** Take as written. Add the facts below to its notes, and to the notes of
  `fork-and-sign-in` and `conversation-resumes-in-a-copy`.
- **Confidence:** high. It rests on both vendors' install and login pages.

### What it decides, on one real case

In main, over `ssh main`, you run `curl -fsSL https://claude.ai/install.sh | bash`. Claude Code
installs into `~/.local/bin/claude` -> `~/.local/share/claude/versions/<v>` and updates itself in
the background. You run `claude`. It cannot open a browser on your Mac, so you copy the link,
sign in on your Mac, and paste the code back at `Paste code here if prompted`. The macOS Keychain
in main is locked in an SSH session, so Claude Code stores the login in
`~/.claude/.credentials.json`. For Codex, you run `brew install --cask codex` or
`npm i -g @openai/codex`, then `codex login --device-auth`, or `ssh -L 1455:localhost:1455 main`
and `codex login`. Codex stores its login in `~/.codex/auth.json` by default. Nothing is
modified, and nothing needs Silo.

### Options

The notes already give both options. Nothing found changes them.

### Evidence

- **Verified (vendor page).** Claude Code: the native installer updates in the background. The
  default channel is `latest`. Homebrew does not update unless `CLAUDE_CODE_PACKAGE_MANAGER_AUTO_UPDATE=1`.
  `DISABLE_AUTOUPDATER` and `DISABLE_UPDATES` exist. It needs macOS 13 or later and 4 GB of
  memory. https://code.claude.com/docs/en/setup
- **Verified (vendor page).** Claude Code on macOS: "When the Keychain rejects the write, such as
  when it's locked in an SSH session, Claude Code stores your login in
  `~/.claude/.credentials.json` with file mode `0600` instead". `claude setup-token` makes a
  one-year token that "can only make model requests".
  https://code.claude.com/docs/en/authentication
- **Verified (source).** Codex's default is to keep the login in a file: `AuthCredentialsStoreMode`
  has `#[default] File`, "Persist credentials in CODEX_HOME/auth.json". MCP sign-ins default to
  `Auto`, which prefers the Keychain (`codex-rs/config/src/types.rs`, commit `75df509`). So in a
  macOS computer, an MCP server's sign-in made through Codex goes to the Keychain.
- **Verified (source).** Codex checks GitHub releases at startup and offers the upgrade command of
  however it was installed (`codex-rs/tui/src/updates.rs`), unless `check_for_update_on_startup`
  is off.
- **Verified (vendor page).** Codex: device-code login "must be enabled in your ChatGPT security
  settings"; the fallback is `ssh -L 1455:localhost:1455`. https://learn.chatgpt.com/docs/auth.md
- **Verified (vendor page).** The Codex CI guide: "Do not share the same file across concurrent
  jobs or multiple machines". It also says to sign in again if "another machine or concurrent job
  rotated the token first", or after a "round trip that restores an old file".
  https://learn.chatgpt.com/docs/auth/ci-cd-auth. A restore to a checkpoint restores an old file.
  A fork is a second machine with the same file.

### Risks and what would change this

- After `silo restore main <checkpoint>`, `claude --resume` and `codex resume` may first need a
  new sign-in, because the login on the disk is the old one. That does not make the restore
  wrong. But the first build's story ("you take Claude Code's conversation up again with
  `claude --resume`") should say it. `conversation-resumes-in-a-copy` should count a new sign-in
  after a restore as expected, not as a failure.
- A restore also puts back the program version of the checkpoint. The native installer then
  updates again on its next start. That is harmless, and worth one line in the restore output.

### For the first build

It blocks `agent-programs-run-unmodified`. Nothing more is needed from it. The builder needs the
install path above to work in main: network out to `claude.ai`, `downloads.claude.ai` and
`github.com`, which comes back to main's network in the first build.

## `silo-is-light`: Is "What Silo adds costs as little as it can" a principle of Silo?

- **Recommendation:** Take with an edit. Keep the principle. Make "as little as it can" measurable
  by naming the scale: what the computer itself costs on the reference machine.
- **Confidence:** medium. The VM numbers are published by one careful source, on one machine. I
  could not measure Silo-like processes here: my run of gvproxy to read its idle memory was
  refused by this environment's permission check.

### What it decides, on one real case

Main is a macOS computer with 8 GB given to it. Published figures put its idle use near 5 GB, and
a basic install near 54 GB of disk. Silo adds an engine, the SSH helper above, the network stack,
checkpoints every few minutes, and a comparison of two checkpoints for `silo changes`. "Light"
then means each of these is small against 5 GB and 54 GB, and adds no wait you notice to `ssh
main`, `git clone` or a write inside main.

### What "light" can mean, in numbers

| Cost | Published figure | Source |
|---|---|---|
| Memory of a macOS computer, given 4 / 6 / 8 GB | 3.1 / 3.9 / about 5 GB used | Eclectic Light, macOS 26.4.1 on a Mac mini M4 Pro (verified, article) |
| Smallest usable macOS computer | 2 cores and 4 GB; Tahoe cannot run on one core with 3 to 4 GB | Eclectic Light (verified, article) |
| Disk of a basic macOS computer | about 54 GB for a 100 GB disk; under 50 GB it cannot update macOS; 60 GB recommended | Eclectic Light (verified, article) |
| Apple's restore image, macOS 26.6.2 | 19,772,231,540 bytes (19.8 GB) | `Content-Length` of the image Lima pins (verified, HTTP request) |
| Tart's defaults for a macOS VM | the restore image's minimum memory, at least 4 cores, 50 GB disk | Tart source, `Create.swift`, `VM.swift` (verified) |
| VS Code Server in the remote | 1 GB of memory required, 2 GB recommended | VS Code docs (verified) |
| Claude desktop's helper in the remote | about 1 GB of disk per user | Claude desktop docs (verified) |
| SSH through vsock vs a user-space network (Linux guest) | 3.57 vs 2.34 Gbit/s | Lima PR 3979 (verified) |

Sources: https://eclecticlight.co/2026/05/02/how-fast-is-a-macos-vm-and-how-small-could-it-be/,
the IPSW URL in Lima's `templates/_images/macos-26.yaml`.

From this, a target that reads as "light" against the computer is a few percent of its idle
memory, so tens of megabytes for Silo's processes per computer. Idle processor near zero, meaning
no polling loop. The setup of a connection adds no wait you can tell from plain ssh. These are
targets for `own-cost-targets` to set, not for this principle. The principle should only say what
they are measured against.

Two costs on this list are Silo's own and large. The plan should not file them as the VM's:

- **Checkpoint disk and stalls.** PR #347 measured 11 to 23 s of stalled writes on XFS from one
  reflink copy of a disk image in about 500,000 extents. On a Mac the APFS clone is to measure.
  Checkpoints are an addition of Silo's, so they fall under this principle.
- **The network stack's slowdown.** `network-stack` puts every packet of main through Silo. Its
  cost in `git clone` and `npm install` against Apple's NAT is a "waiting" this principle covers.

### Options

| Option | What it costs | Who does it |
|---|---|---|
| A principle with targets (current answer) | Slower additions, some refused | TigerBeetle's budgets, and the Guillotine notes already in the plan |
| A principle with targets, measured against the computer (proposed) | The same, plus one reference measurement of an idle computer | — |
| Tune when slow | Weight nobody chose | Most desktop VM tools |

### Risks and what would change this

If the reference machine is a large Mac, every target looks small. polarzero's Mac is the natural
first reference, but base Macs now ship with 16 GB (not verified here), and on 16 GB a 5 GB
computer is already a third of the memory. Name a 16 GB Mac as the second reference.

### Proposed text

`answer`:

> Yes. What Silo adds on top of the virtual machines costs as little as it can. Every addition says
> what it adds in memory, processor, disk and waiting, and names its target before it is built.
> A target is measured on a named reference machine, against what an idle computer costs there,
> so it reads as a share of the computer's own cost. What Silo's choices add to a computer's own
> work, such as the wait a checkpoint or the network stack adds inside it, counts as Silo's.

`sets` unchanged: `costs-little`, "What Silo adds on top of the virtual machines costs as little
as it can.", asking "What does it add in memory, processor, disk and waiting, and what is its
target?"

### For the first build

It blocks it, through `own-cost-targets`. The first build needs one measurement before any
target: an idle main on polarzero's Mac (memory, disk, processor), taken once. Every target of the
first build's features is then written against it.

## Cross-cutting findings

1. **A login in the macOS Keychain cannot be kept out of a fork.** `fork-and-sign-in` says a fork
   is made without the sign-ins agent programs keep on the disk. On a macOS computer, where a
   login lands depends on how you signed in. Claude Code signed in over SSH writes
   `~/.claude/.credentials.json`, because the Keychain is locked. Signed in on main's screen, it
   writes the login Keychain. Codex's MCP sign-ins prefer the Keychain. The Keychain is one file,
   `~/Library/Keychains/login.keychain-db`, holding every item, and `documented-surfaces` forbids
   reading its items' private names. So Silo can keep a login file out of a fork but not one
   Keychain item. `fork-and-sign-in` should say so for macOS computers: such a login is copied,
   and Silo says so before the fork is made. The fork is not in the first build, so this blocks
   nothing now.

2. **A restore brings back an old login.** Codex writes a new login to its file on each refresh,
   and its guide names "a round trip that restores an old file" as a reason to sign in again.
   Whether Claude Code's refresh makes the old login unusable was not checked. The
   first build's main promise is a restore. Its story should say you may sign in again after a
   restore, and `conversation-resumes-in-a-copy` should expect that.

3. **The way in should be a fact on the Mac, not a report.** Several open entries need to know
   "is the way in open, and by which program": `computer-reached-over-ssh` showing VS Code's link,
   `reaching-a-computer` showing the grant among channels, and `following-the-work`. A Unix
   socket per computer plus `LOCAL_PEERPID` makes that a fact read outside the computer, with
   nothing inside macOS. This is the one design point I would add to `network-stack` or to the
   first build's feature. A `localhost` port cannot give it reliably.

4. **Claude desktop's way in carries your accounts into the computer.** Over SSH, Claude desktop
   forwards nothing of your Mac, which is good. But it renews your Claude sign-in for the task
   inside the computer, and delivers your claude.ai connectors, such as mail and drive, to Claude
   Code there. An agent in main with every check off can then use your connected accounts through
   Desktop. This is a channel to the internet with your identity. It is not on the list of seven
   channels (`channel-list`). The channels decision should name it when Claude desktop is used.

5. **The first build has an open network and no rule, which the plan does not say.** "Nothing
   else is open in a new computer until you open it" (`reaching-a-computer`), the principle
   `every-crossing-is-a-channel` ("none exists by default") and `network-stack` all assume rules.
   The first build leaves out rules. Main must still reach `claude.ai`, `github.com` and npm on
   its first day. The first build should say plainly: main's network is open from the moment you
   make it, with your yes when you make it, and shown as a channel.

6. **Challenge to `channel-list` (answered, not taken).** The list puts "the clipboard" among the
   channels as something Silo grants and rules. The terminal over SSH already gives a computer a
   way to write your Mac's clipboard (OSC 52), in Ghostty by default and in other terminals by
   setting. Silo cannot see or rule it. Since `holes` says Silo never fills a hole, the list
   should say that this one is outside Silo and depends on your terminal's setting, rather than
   imply that the clipboard only crosses when Silo opens it.

7. **Mac tests this review adds** to the eight in the first-build proposal:
   - Whether the Codex app lists hosts from an `Include`d file.
   - Whether Claude desktop and the Codex app connect through `ProxyCommand` with `ProxyUseFdpass`.
   - Turning on Remote Login in a fresh macOS disk before its first start, as Lima does.
   - The peer pid of a Unix socket connection, walked to the app that started ssh.
   - The idle memory and disk of main on polarzero's Mac, as the reference for every target.

## What I could not check

- Nothing was run on a Mac. Every macOS behaviour here comes from vendor pages, Lima's and Tart's
  source, or published measurements.
- VS Code Remote-SSH's own handling of agent forwarding (`remote.SSH.enableAgentForwarding`):
  the extension is not open source, and its pages I read do not describe it.
- My measurement of gvproxy's idle memory, as a stand-in for Silo's network stack, was refused by
  this environment's permission check. It is not reported.

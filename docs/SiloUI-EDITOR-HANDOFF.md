# Native editor and browser handoff

Network links use the current browser preference: Launch Services (`open -a`) on macOS and GIO `launch_uris` on Linux. Only HTTP(S) URLs without embedded credentials are accepted. URL contents are positional arguments, never shell programs. Explicit unavailable preferences fail instead of opening a different app.
Linux Browser choices require a `.desktop` entry, matching the [GIO desktop-file
constructor](https://docs.gtk.org/gio-unix/ctor.DesktopAppInfo.new_from_filename.html)
used by the launcher. Both Choose and an existing browser preference report this
requirement for executable-only paths. GIO still parses and validates the entry
before use. Terminal and editor executable choices use their existing adapters. The
Linux-only `browser_desktop_selection_reaches_a_fake_launch_and_rechecks_removed_targets`
regression launches a disposable script through GIO, verifies literal URL arguments,
and rejects removed executables and desktop files. Portable preference tests cover
the system default. This does not qualify an installed browser or an interactive
Linux desktop.

Folder actions use MicroSandbox's SSH server over standard input/output, not a `/workspace` path on the device. The app checks the managed computer is running and the requested directory exists before opening it. `msb ssh serve --stdio --no-start` prevents the editor transport from starting a stopped computer, including a stop between checking and connecting.

Guest folder names are raw filesystem paths. URI handoff encodes each segment with [`Url::path_segments_mut`](https://docs.rs/url/2.5.8/url/struct.Url.html#method.path_segments_mut), including literal percent signs: a folder named `some%20comments` is sent as `some%2520comments` so the editor decodes the original name. `Url::set_path` accepts existing percent escapes and therefore cannot preserve every raw folder name.

Editor handoff rejects ASCII control characters in folder names. The URL parser discards tabs, carriage returns, and newlines instead of encoding them, so accepting those names would open a different path. Ordinary spaces and Unicode names remain supported.

Silo creates an Ed25519 client identity in its private runtime home, adds only its public key to MicroSandbox authorization, and pins the computer's locally stored SSH host public key. The guest does not receive either private key. SSH uses `IdentitiesOnly`, disables the agent, and requires the pinned host key. Each exact computer alias has a private configuration file. One `Include` line is prepended to the user's SSH configuration; the existing bytes are preserved and repeated actions do not duplicate the include. A symlinked `~/.ssh` or `~/.ssh/config` (stow, chezmoi) is followed to the folder or file it points to when this account owns it, and the file is replaced atomically so the link stays in place. When the target can't be changed (for example a read-only home-manager file in the Nix store) or the link is broken, the error names the exact `Include` line to add by hand; once it is there, Silo writes nothing. Oversized or non-regular configuration files still fail explicitly. Terminals for remote computers pass `-F` and never touch `~/.ssh/config`.

Reusing a connection key repairs group or other access to `0600` through the
existing atomic private-file writer, preserving the key bytes and identity.
[OpenSSH's key loader](https://github.com/openssh/openssh-portable/blob/V_9_9_P2/authfile.c#L86-L111)
rejects account-owned private keys with any group or other permission bits, even
when their parent directory is private. Already private keys need no rewrite.

Supported adapters:

- Zed: bundled CLI on macOS or discovered executable on Linux, with an encoded `ssh://alias/path` URI (the user comes from the alias's SSH configuration).
- Visual Studio Code: bundled CLI on macOS or discovered executable on Linux, with `--profile <channel profile> <workspace file>` (see [VS Code and the computer](#vs-code-and-the-computer)). VS Code needs its Remote - SSH extension in the current channel's profile.
- Other editors: explicit unsupported message. No successful-looking local folder fallback.

Settings list only editors and terminals Silo can hand a computer to, and a system default that cannot (for example Xcode as the `.swift` handler, or a plain text editor as the Linux `text/plain` handler) falls back to the first supported app (G-07). Choose… still accepts any application and reports the unsupported message when used. macOS editors: Visual Studio Code (and Insiders) and Zed (Preview, Nightly, Dev); terminals: Terminal, Ghostty (through `osascript`, off the main thread) and iTerm.

On Linux the command comes from the desktop entry's `Exec`, parsed with GLib (G-06, G-25): an `env NAME=value` prefix (snap) is skipped; the Microsoft package's Electron binary (`/usr/share/code/code`) is replaced by its CLI `bin/code`; Zed's tarball `libexec/zed-editor` by `bin/zed`; Flatpak entries (`X-Flatpak=com.visualstudio.code` or `dev.zed.Zed`) run `flatpak run <app id>`. A launcher that has not exited after 10 seconds is left running rather than killed; only a launcher that exits with an error reports a failure. Flatpak editors still need their sandbox to reach the device's `ssh` and Silo's ProxyCommand paths; that is not verified.

Linux terminals with a command launcher: GNOME Terminal, Console (`kgx`), Ptyxis, Konsole, Ghostty, Alacritty, kitty, xterm, Tilix, WezTerm, Xfce Terminal, plus the launchers `xdg-terminal-exec` and Debian's `x-terminal-emulator` (`-e`). With the system default selected, Silo uses `xdg-terminal-exec`, then `x-terminal-emulator`, then the first listed terminal. The deprecated GNOME `default-applications.terminal` key is not read: its schema is missing on current systems, and a missing GSettings schema aborts the process.

When Silo runs as an AppImage (G-24), every child it starts from these files (terminals, editors, `ssh`, `ssh-keygen`, browsers through GIO) gets the system environment: variables the AppRun hooks point into the AppImage mount (`GSETTINGS_SCHEMA_DIR`, `GTK_PATH`, `GDK_PIXBUF_MODULE_FILE`, `GST_*`, `LD_LIBRARY_PATH`, and mount entries of `XDG_DATA_DIRS` and `PATH`) lose those entries or are removed, as are `APPDIR`, `APPIMAGE`, `ARGV0` and `OWD`. The helper is `applications::launch::sanitize_child` (and `child_environment` for a GIO launch context); the remaining spawn sites in `app_menu.rs`, `github.rs` and `updates.rs` belong to other packages. The AppImage mount also changes on every start, so editor SSH configurations name the AppImage file: local computers use `ProxyCommand '<AppImage>' --msb-ssh-serve <runtime home> <sandbox>`, which runs the bundled `msb ssh serve` from that run's mount, and remote computers `'<AppImage>' --remote-guest`. At startup an AppImage Silo rewrites the ProxyCommand of configurations written by earlier runs (G-12).

After the storage migration, the entries copied from the previous generation are repointed at the converted runtime home at every launch, on every build, and the converted home's `Include` is added where the user's file still includes the previous one, so an editor that reconnects by itself opens the current computer. See [Editor connections after the migration](SiloUI-RUNTIME-PACKAGING.md#editor-connections-after-the-migration-2026-10-01).

AppImage environment cleanup uses [`std::env::split_paths`](https://doc.rust-lang.org/std/env/fn.split_paths.html) and `join_paths` to preserve native path bytes, including non-UTF-8 system directories. [`Path::starts_with`](https://doc.rust-lang.org/std/path/struct.Path.html#method.starts_with) compares components, so repeated and trailing slashes still identify bundled entries while similarly named sibling mounts remain untouched. Pure launch-policy regressions cover these cases; they do not qualify a live Linux desktop launch.

Terminal shell commands require UTF-8 runtime home, executable and library paths.
Each is checked with `Path::to_str` before the command is written; display text is
never used for environment assignments. Command-boundary tests preserve literal
spaces, quotes and shell characters and reject non-UTF-8 bytes in each path.

## Trust the editor gives the computer

An editor's remote mode runs a server inside the computer that talks back to the editor on this device. Treat that server as code running in the computer: the working account has `sudo`, so anything in the computer can replace or drive it. The measures below narrow what the editor offers it; they do not make the editor a security boundary.

### VS Code and the computer

Owner decision 5 of the [2026-09-29 review](research/codebase-review-2026-09-29.md) (G-19). Silo opens every computer folder in VS Code with a dedicated profile and a workspace file that Silo writes on this device. `Channel::vscode_profile()` and `Channel::state_dir()` in [`channel.rs`](../app/SiloUI/host/src/channel.rs) supply the profile and private home; see [build channels](SiloUI-BUILD-CHANNELS.md). Production names stay unchanged, and Dev uses its own profile and home:

- `code --profile "<channel profile>" "<channel home>/editor/<alias>/<path hash>/<folder>.code-workspace"`. The file names the remote folder (`vscode-remote://ssh-remote+<alias>/<path>`) and `"remoteAuthority": "ssh-remote+<alias>"`, which VS Code uses to open a local workspace file in a remote window (`windowsMainService.ts` resolves it through `resolveLocalWorkspace`).
- Its workspace settings are `github.gitAuthentication: false` (the computer's Git cannot borrow the GitHub session of VS Code on this device), `git.terminalAuthentication: false` (computer terminals get no `VSCODE_GIT_IPC_HANDLE` askpass route back to this device), `remote.autoForwardPorts: false` and `remote.forwardOnOpen: false` (computer ports reach this device only through Silo's port publishing). All four are window- or resource-scoped, so workspace settings apply. Silo rewrites these keys, the folder and the remote authority on every open and keeps any other workspace settings the user added.
- Silo currently reads workspace files as strict JSON objects. Although [VS Code supports comments in workspace files](https://code.visualstudio.com/docs/editing/workspaces/multi-root-workspaces#_workspace-file-schema), Silo leaves a commented, malformed, or non-object workspace file unchanged and reports its path instead of replacing the user's configuration. Remove comments or repair the JSON before reopening through Silo.
- Workspace settings are used instead of settings in the profile because VS Code only creates a profile when a window opens with it (`--profile` creates a missing profile empty; `--install-extension --profile` fails for a missing profile), so the first window would otherwise run without them. Workspace settings also outrank the "Remote" settings file that the computer itself can write (`~/.vscode-server/data/Machine/settings.json`); user and profile settings do not.
- The profile keeps the user's normal profile untouched and limits which extensions reach the computer to those installed in the current channel's profile. On first use VS Code creates the profile empty and shows "Extension 'Remote - SSH' is required to open the remote window. Do you want to install the extension?"; **Install and Reload** installs it into the current channel's profile only (`nativeExtensionService.ts` `_handleNoResolverFound`). Themes and other extensions can be added to that profile in VS Code's Profiles editor.

What remains trusted (from VS Code's architecture and documentation; not tested against a hostile computer):

- A computer can edit `.vscode/settings.json` in the opened folder. In a workspace window those folder settings can still turn resource-scoped settings such as `github.gitAuthentication` back on.
- The VS Code server and the extensions running in the computer use VS Code's extension API, which the window on this device serves. That includes reading and writing this device's clipboard (`vscode.env.clipboard`), opening links in this device's browser (`vscode.env.openExternal`), forwarding ports through the tunnel API even with automatic forwarding off (VS Code's own `remote.autoForwardPorts` description says extensions can still forward ports), showing prompts and notifications in VS Code, and asking for authentication sessions (`vscode.authentication.getSession`). A session for an account the user already allowed for an extension ID can be handed to code that claims that ID, because the computer's extension host reports the IDs itself. Sign in to GitHub or Copilot in the current channel's profile only if the computer may use that account.
- Anything typed into a prompt that a computer extension shows goes to the computer.
- VS Code's Workspace Trust and "Restricted Mode" limit what the workspace's own tasks and settings do, not the server.

Opening a computer folder from VS Code's own recent list or Remote Explorer skips Silo's workspace file and uses whichever profile that window had; open computer folders from Silo to get these settings.

### Zed and the computer

Checked against Zed's [remote development documentation](https://zed.dev/docs/remote-development) and the `zed-industries/zed` protocol definitions on 2026-09-30 (not tested against a hostile computer):

- The UI, language-model requests and their credentials stay on this device; language servers, tasks and terminals run on the server in the computer.
- Port forwarding is not automatic: Zed forwards only the ports listed in `port_forwards` of the user's `ssh_connections` settings. Silo sets none.
- Zed holds no Git credentials for the computer. When Git in the computer asks for credentials, the server sends an `AskPassRequest` whose prompt text comes from the computer (`crates/proto/proto/app.proto`); Zed shows it and returns whatever the user types. Treat such prompts as coming from the computer.
- Project settings (`.zed/settings.json` in the computer folder) are read by both sides, so the computer can change Zed's behaviour for that project.

No Zed-specific settings are needed for decision 5; Silo keeps launching Zed with its `ssh://` URI.

### Terminals and the computer

Open terminal runs `msb exec --tty` (local computers) or `ssh -t` (remote computers) in the chosen terminal app, so the computer's output reaches that terminal byte for byte (G-21). Filtering it would need a terminal emulator between the two and would break full-screen programs, so Silo does not. Escape sequences that programs in the computer can send include:

- **OSC 52** writes this device's clipboard (and, in terminals that allow it, reads it).
- **OSC 8** hyperlinks show one text and open another address.
- **OSC 0/2** set the window or tab title, which can imitate another app or prompt.
- Report requests (title reporting, some device status queries) make the terminal type a reply into the input of the program running in the computer.

Recommended settings for a terminal used with Silo, from each terminal's documentation (check the names in your version): Ghostty `clipboard-write = deny` (or `ask`), `clipboard-read = deny` and `title-report = false`; kitty `clipboard_control` without `write-clipboard`/`read-clipboard`; Alacritty `[terminal] osc52 = "Disabled"`; WezTerm and Konsole: disable OSC 52 clipboard access where the version offers it; iTerm2: leave **Applications in terminal may access clipboard** off and **Terminal may report window title** off; xterm: keep `SetSelection` and the title reports in `disallowedWindowOps`. Hover a link to see its real address before opening it. A dedicated terminal profile for Silo sessions keeps these settings away from your other work.

The device needs its standard OpenSSH client (`ssh` and `ssh-keygen`); missing executables produce an explicit error. The bundled runtime SSH session resolves the guest login home once and uses it for both shell commands and SFTP, so editor uploads and remote mkdir commands agree.

The transport is probed before launching. Launch success means the operating system/editor CLI accepted the handoff; editor server installation, extensions and guest network requirements can still fail inside the editor.

Primary references:

- [Zed SSH remote development](https://zed.dev/docs/remote-development): SSH URI support and use of the user's SSH configuration.
- [VS Code remote CLI](https://code.visualstudio.com/docs/remote/troubleshooting#_connect-to-a-remote-host-from-the-terminal): remote folder URI and Remote SSH behavior.
- [VS Code profiles](https://code.visualstudio.com/docs/configure/profiles) and `microsoft/vscode` sources read 2026-09-30: `src/vs/platform/environment/node/argv.ts` (`--profile` creates a missing profile empty), `src/vs/code/node/cliProcessMain.ts` (extension commands reject a missing profile), `src/vs/platform/windows/electron-main/windowsMainService.ts` (local workspace files carry `remoteAuthority`), `src/vs/workbench/contrib/remote/common/remote.contribution.ts` (`remote.autoForwardPorts`, `remote.forwardOnOpen`, window scope), `extensions/git/package.json` and `extensions/github/package.json` (setting scopes).
- [GIO AppInfo.launch_uris](https://docs.gtk.org/gio/method.AppInfo.launch_uris.html): application-aware URI launching and desktop-entry handling.
- Bundled MicroSandbox source, pinned commit `5eca4de8bf233e57f114140f8c076ea8c96f21ab`, `crates/cli/lib/commands/ssh.rs` and `sdk/rust/lib/sandbox/ssh.rs`: stdio transport, authorized key store and per-computer host key path.

Focused tests: `cargo test --manifest-path app/SiloUI/src-tauri/Cargo.toml editor::tests` and `applications::tests`. The ignored `editor::tests::live_editor_transport` requires `SILO_LIVE_TEST_CONFIRM=disposable-test-fixtures` and explicit `SILO_EDITOR_USER_HOME`, `SILO_EDITOR_RUNTIME_HOME`, `SILO_EDITOR_MSB`, and `SILO_EDITOR_LIBRARY`; the fixture home must already exist and must not resolve to `$HOME`; it modifies that home's SSH integration and proves the dev test repository is reachable. `SILO_EDITOR_OPEN_ZED=1` also launches the real editor. It is never enabled during ordinary tests or production startup.

## Live macOS verification (2026-09-10)

The focused live test passed against the running `dev` computer with the patched bundled CLI: SSH `pwd` equals `$HOME`; a shell-created relative directory receives an SCP/SFTP marker; the transfer exits zero; SSH reads identical marker bytes; the test deletes its marker and directory. Real Zed then completed server installation, opened `silo-files-test-express`, and displayed the repository's `Readme.md` from `/workspace/silo-files-test-express/Readme.md`. No repository file was edited.

This exposed and fixed two runtime bugs, covered by the live transfer regression: SSH/SFTP previously used `/` instead of the login home, and SFTP channel completion omitted SSH exit status (OpenSSH SCP returned failure despite successfully transferring bytes). The patch sets a shared login-home cwd and sends subsystem completion status on client EOF. Neither fix uses Zed-specific paths.

VS Code and Linux editor opening still require real-platform verification; URI/configuration and preference validation tests do not substitute for those checks. For the current channel's profile (G-19) the macOS check is: the first open creates that profile and offers Remote - SSH, the window opens the computer folder with the four workspace settings in effect, the user's default profile settings file is unchanged, and a second open reuses the profile without a prompt.

## Configuration repair failures

Startup repair reports per-file read and replacement failures through the existing
editor connection warning. It repairs writable entries in a partial batch, preserves
files whose atomic replacement fails, and adds the converted runtime's Include
only after every entry succeeds. Repeating repair after storage becomes writable
updates ProxyCommand, IdentityFile and UserKnownHostsFile before reporting success.
An absent optional SSH configuration directory is harmless. AppImage transport
refresh reports the same incomplete rewrite errors.

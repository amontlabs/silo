# silo-ui

## 0.11.0

### Before upgrading

- **Debian package users:** Silo's APT repository moved. Run this once, then update as usual:
  `sudo sed -i 's#https://0xpolarzero.github.io/silo/apt#https://apt.silo.amontlabs.com/apt#' /etc/apt/sources.list.d/silo.sources`
- **GitHub access:** the Silo GitHub App is now `silo-amont-labs`. Earlier versions can't find it, so update to keep using GitHub repository access.
- **Remote computers:** update Silo on every computer you manage remotely; this release changes the remote protocol.

### Minor Changes

- 363cd18: Agents installed later in a sandbox are now set up for computer use automatically. Claude Code and Codex are registered up front, and Pi, OMP and Hermes are registered as soon as you install them, so the "Set up computer use for new agents" menu item is gone.
- 2e05df9: Silo now prepares the VM image, the LCU archive and ChatGPT for Linux in the background at launch, with one non-blocking notification that shows progress, offers Retry when something fails, and stays out of the way once everything is ready.
- ff2cbc6: New sandboxes use the v4 guest image, which includes the built-in Linux desktop, and get agent computer use with no setup. Silo downloads the official ChatGPT app for Linux from OpenAI by itself in the background on every computer that runs it (nothing to accept, retried automatically, never blocking sandbox creation or start) and shares it read-only with that computer's sandboxes; each sandbox installs LCU against it at boot, so computer use becomes ready automatically once that one-time background download and the sandbox's setup complete, and Claude Code, Codex and other agents can then use the desktop. Settings, Computers shows the download state on each computer and offers Retry after a failure. A per-sandbox switch lets computer-use actions run without asking first, and "Set up computer use" reruns setup after you install a new agent. Sandboxes created before this version keep their current desktop; create a new sandbox to use computer use.
- 18e6df1: Silo now calls the environments agents use "computers" and the Macs and Linux machines running Silo "devices", and remote management is now "Connections". SSH-only computer entries are no longer offered, and any saved ones are dropped. Saved data is converted automatically on first launch. Connected devices must run the same Silo version, and when they do not, the message names the device to update. Exports from earlier versions cannot be imported.
- fdb04bd: Settings → Computers no longer lists ChatGPT for Linux for every computer. Silo still downloads it in the background; a **Computer use components** section now appears only when a computer's download fails or its status cannot be read, with **Retry** or **Refresh**. A new switch, **Allow agents to use the computer without asking in new sandboxes**, sets the starting choice for sandboxes you create or import (off by default).
- b402a48: Creating a sandbox now finishes everything, so **Created** means it is ready and the first start is just a start. The creation notification waits for the VM image and for ChatGPT for Linux (with its download progress) without blocking other sandbox actions or Quit, then sets up the desktop and computer use. If the download fails you can retry or finish without computer use, and anything left over completes at first start.
- 2426f41: Remote management keys no longer carry owner forwarding privileges, and published ports on remote computers now go through guest SSH. This changes the remote protocol: a computer running this version cannot be managed by, or manage, a computer running an older Silo, so update Silo on both computers before reconnecting.
- 3d5600f: New Linux desktops no longer install Luda agent tools, and the desktop viewer no longer offers "Repair agent tools". LCU is the supported computer-use integration. New v4 sandboxes set it up automatically; for a desktop created before v4, set LCU up from the running desktop. Existing desktops that already have Luda are left untouched.
- c15a9b7: Sandboxes on other computers can now be dragged into place in the sandbox list too. Each computer keeps its own order of the list, for its own sandboxes and remote ones, and reordering no longer changes the sandbox configuration. Sandboxes that need attention still stay at the top. A sandbox's ⋯ menu also has **SSH**, after **Storage**, to open its SSH tab.
- 7c13124: Creating a sandbox now shows its current step in the notification from the start, including the one-time image preparation on a new computer. When it finishes, the notification says the sandbox was created and lets you turn on **Allow without asking** for computer use right there.
- 6f8e47e: A sandbox's computer use is now just the "Allow without asking" switch, which works whether or not the sandbox is running. Status, versions and download progress appear only when something goes wrong, with Try again or Retry. "Set up computer use for new agents" moved to the sandbox's actions menu, and the sandbox editor no longer describes the built-in desktop.

### Patch Changes

- 01fdaec: Avoid starting an export-file check after its import request has already been cancelled.
- 169e92e: Reduce repeated accessibility checks when a sandbox's accessibility service is unavailable, and resume frequent checks when application content changes.
- 39a836f: Keep Start, Stop, Restart, and Quit working when activity history cannot be read or saved. Show a warning in Activity, preserve damaged history, and clear completed or cancelled actions independently of history writes.
- 79a39a4: A sandbox row's status, message and **Dismiss** button now line up on one line instead of sitting at slightly different heights.
- 17600b2: Keep all system notification messages on one line and within 200 characters.
- 2ab3446: Cancel system notices invalidated by sandbox deletion while notification policy is being checked, before submitting them to the OS.
- 42b20a5: Recheck window focus and notification preferences after queued notifications finish waiting, so disabled notifications do not submit using stale settings.
- 4d3e17c: Keep newer system notifications from being replaced by delayed older results, and prevent duplicate notifications for the same action.
- 527ae7a: Remove system notifications when their sandbox is deleted, including notifications still waiting to appear.
- 2178cd6: Keep older notifications withdrawable when a replacement is skipped because system notification permission or the desktop notification service is unavailable.
- ff493c1: Prevent AppImage libraries from interfering with external Linux applications, even when folder paths contain unusual characters or repeated slashes.
- 1052cc8: Keep your latest application preference when an older app chooser finishes, and ignore results after you leave the settings.
- be72a8b: Keep newer system application defaults when an older application discovery response arrives later.
- 1510ec7: Preserve Linux editor launch options when the launcher separates options from file paths.
- bdd8267: Preserve environment assignments and isolation options from Linux editor desktop entries when opening a sandbox.
- 1e96618: Include Linux editor and terminal launchers that clear selected environment variables.
- 9e62895: Preserve the selected Flatpak editor's branch, architecture, installation, and command when opening a sandbox.
- 9ad90fa: Open the selected Ghostty app when multiple copies are installed.
- 89d1d34: Report an unavailable editor when its macOS bundled command has lost execute permission.
- e808832: Preserve Linux editor desktop-entry options, including isolated user-data and extension directories, when opening a sandbox.
- 94734eb: Keep editor SSH aliases available when the runtime directory contains brackets, question marks, asterisks, or backslashes.
- 3e38bba: Exclude Linux editor entries whose resolved command launcher is missing or no longer executable.
- 330a974: Stop suggesting Linux terminals whose command has been removed or lost execute permission.
- e4a80d7: Preserve installed desktop services and account configuration when a migration file write is interrupted, and flush generated files before completing setup.
- ef546c7: Allow home migration to retry interrupted file copies without leaving incomplete credentials or launchers at their final paths.
- 309a5f6: Preserve existing shell files during interrupted home migration and avoid changing files outside the new home through hardlinks.
- 60abee0: Clear notifications when their sandbox is deleted, even when another computer has a sandbox with the same name.
- f144534: Keep delayed export or import cancellation from marking a later transfer as cancelled after relaunch.
- b729dc2: Skip pending import file checks after you close the review, and keep them from interfering with a new review.
- 07d1680: Keep each export's result separate so a new export cannot return an earlier file.
- da51ab3: Keep imported disks and recovery records when a settings write fails and Silo cannot verify whether the sandbox was saved.
- 653e32e: Keep the pre-upgrade backup removed from Storage when an older status read finishes after deletion.
- 4fb3b94: Wait for the actual backup deletion result when Retry overlaps a deletion already in progress, instead of reporting success early.
- 93313e8: Ignore oversized saved export-folder history without using excessive memory or changing backups and unfinished transfers.
- 2723d87: Keep Storage accurate when a backup is deleted while Silo starts.
- dc9e866: Show backup refresh failures in Storage and make Retry restore automatic updates.
- 2ef2625: Stop refreshing backup and migration results for closed views, even if their updates are still connecting.
- cfa508e: Reject export and import selections with unsupported filenames instead of opening a different path or remembering an unusable export folder.
- c90ff75: Keep published sandbox exports when the destination folder's durability check fails, and preserve files another writer places at the export path.
- c5ac575: Keep pre-upgrade backups available for manual deletion when their saved deletion dates exceed the supported date range.
- b48e58c: Keep pre-upgrade backups available when their retention record is oversized, redirected, or a special file, without blocking backup status or automatic cleanup.
- 28f31a6: Protect existing files when exporting to shared folders. If a filesystem cannot safely publish a new export, Silo asks you to choose another filesystem.
- 9b2ec1a: Keep log browsing responsive with a bounded history window, preserve older paging and full export, and prepare copied log text only when Copy is clicked.
- 5fccbba: Reuse repository search results when the catalog, selected repositories, and search text have not changed.
- cd6add4: Name new Silo Dev editor SSH keys after the development channel while preserving existing keys.
- 16846bc: Use the Silo Dev name in system menus, dialogs, the Linux tray, and shutdown messages.
- 4a57e41: Reclaim ChatGPT app files left behind by interrupted version deletion.
- 92d3c18: Recover from invalid ChatGPT app publication records without blocking status reads or downloads.
- a326a23: Keep your ChatGPT download retry request when the previous failed download is still stopping.
- 6335bd1: Refuse a symlinked shared ChatGPT folder during sandbox creation even while a download holds the storage lock.
- 56a132c: Back off remote ChatGPT status checks when a computer returns an unreadable response, and resume normal polling after recovery.
- 6de6fcd: Read remote ChatGPT app download status without waiting for local event registration.
- b5c4a62: Keep failed ChatGPT app retry messages visible until dismissed, including when download status becomes ready or unknown.
- 300b5d5: Clean up unfinished extraction when ChatGPT package unpacking fails to start.
- 83b99c5: Clarify that checkpoints must be deleted in Silo on the sandbox's owning computer.
- e65b64a: Restoring a checkpoint after changing your Git identity in Silo no longer brings back the old author. Imported sandboxes keep their existing Git identity until you set one in Silo.
- d83ff53: Keep scheduled storage reclamation running for healthy sandboxes while retrying unresolved checkpoint cleanup for other sandboxes.
- fb8bda4: Prevent a checkpoint failure’s Retry action from starting another checkpoint operation on a sandbox that is already busy.
- 1f31ca1: Keep other sandboxes' latest state when a checkpoint operation finishes, while still showing newly created forks.
- cd69644: Use fewer sandbox status checks when calculating checkpoint storage usage.
- 793696f: Return focus to command search when cancelling a command confirmation.
- c490a3f: Return to the previously focused control when dismissing Commands opened with a keyboard shortcut or native menu.
- 1b83949: Return keyboard focus to Connect computer after cancelling or completing the connection form.
- edd89e3: Show newly connected computers even when an older computer-list request fails during the connection.
- 8fed8cb: Keep Retry on a failed computer removal working after an unrelated computer setting changes.
- 94c7ccf: Ignore obsolete computer-settings Retry actions after newer changes or closed settings controls.
- caa4b2a: Prevent computer-setting retries from overlapping a pending remote-management change or connection removal.
- 28e7cbf: Keep long computer addresses and status messages inside settings rows, and show complete computer names on hover.
- 75c9e90: Show the active SSH repair step when connecting a computer, instead of displaying “Connecting…” during key setup or Terminal authorization.
- 504a33c: Changing a sandbox's computer-use "Allow without asking" setting now returns at once and applies in the background: the panel shows "Applying…", and if it fails or only some agents change it says so and warns that some agents may still act without asking, and Silo tries again when the sandbox starts. Importing or transferring a sandbox starts from Silo's default, a failed command no longer shows an out-of-date setting, a stop or quit interrupts a running change, and a failed ChatGPT download no longer hides the confirmed setting. The switch configures the agents' approval prompts; it is not a security boundary inside the sandbox. Also: recovering an interrupted sandbox creation keeps its original settings, built-in desktops always start with their sandbox, and the desktop viewer no longer offers computer use setup after a failed ChatGPT download.
- 7437ff7: Recheck computer-use readiness after each sandbox boot and repair failed desktop sessions without reinstalling unchanged components.
- ad29396: A sandbox's computer use now says when it is waiting on a failed ChatGPT for Linux download rather than on its own setup, and offers Retry for that download right there. After "Set up computer use" succeeds, the sandbox's page reminds you to reconnect agent sessions to load the tools. Settings, Computers marks a status it could not refresh as last known and offers Refresh.
- 726467d: Save computer-use setup progress to disk before reporting completion, and report errors when it cannot be saved.
- e5f0018: Manual computer-use setup now applies the latest approval choice before finishing when the choice changes during setup.
- 1846c9a: Forking a sandbox now fails cleanly if Silo cannot save its inherited computer-use approval setting.
- a22b03e: Computer use setup retries automatically when a sandbox's network is briefly unavailable.
- 4f1eff8: Report invalid computer-use settings files without blocking status checks or approval changes.
- ac4c224: Reduce repeated computer-use status checks while a sandbox is unavailable, and restore normal checks after recovery.
- 57284c2: Silo now reports computer-use settings cleanup failures instead of reporting sandbox deletion as complete.
- e8e771c: Reject oversized computer-use settings or status without using excessive memory or changing your saved approval choice.
- 226e506: Pause computer-use and remote ChatGPT status checks in hidden views and refresh when you return, while setup and downloads finish in the background.
- 1c49ee2: Ignore late connection-form completions after leaving the form.
- dfebec6: Keep copy feedback tied to the latest clipboard write and prevent delayed feedback timers after leaving a view.
- d8c7012: The button that creates a sandbox now says Create.
- 33d5968: Keep Debian update downloads available when newer Silo releases are published while your package manager still uses an earlier valid package list.
- 3b31efe: Close Silo safely if a Debian update installs but cannot restart. Reopen Silo to finish the update and restore its sandboxes.
- 0df305a: Report invalid bundled sandbox image files promptly instead of leaving setup checks or preparation waiting indefinitely.
- d759b7f: Report invalid bundled Git and virtual-machine files during Linux integrity checks instead of leaving checks waiting indefinitely.
- 1df7cf2: Report damaged bundled dependency information without hanging or using excessive memory.
- c39c69b: Limit setup-check output so it cannot fill temporary storage.
- 2947f60: Keep completed dependency checks visible when slower checks time out, and offer Retry for the unfinished checks.
- 8f8763c: Avoid starting queued dependency checks after Silo closes.
- 9b7b5a4: Keep remote desktop connections tied to the requested sandbox when a sandbox name is reused.
- d7c0365: Reject desktop connection credentials when the selected sandbox is replaced during lookup.
- 09844ff: Save sandbox desktop preferences to disk before confirming the change.
- d993665: Release desktop viewer connections after two minutes when a page request or upload stalls, so you can reconnect.
- eb0c6cd: Reject desktop status and computer-use approval changes for a replaced sandbox instead of applying them to a new sandbox with the same name.
- 378fa74: Keep the desktop viewer listener available after interrupted or aborted connection attempts.
- 7502c0c: Prevent extra request bytes from being sent with bodyless desktop viewer requests.
- a37abab: Reject malformed desktop HTTP framing and authentication fields that use Unicode whitespace as a delimiter.
- 5d6c434: Handle interrupted desktop viewer reads without disconnecting or losing part of a request or response.
- dd7d2fe: Reject invalid desktop viewer requests before they reach the sandbox.
- 96dbe16: Reconnect desktop viewers when their local HTTP listener stops instead of retaining an unusable display connection.
- 80ea0e9: Reject queued desktop actions when their sandbox was replaced, so they cannot change a new sandbox with the same name.
- 87b25a1: Identify development desktop-viewer windows as Silo Dev in their native titles.
- 1943ff9: Reduce repeated desktop viewer connection checks during failures and resume normal checks after recovery.
- 801fc06: Reconnect sandbox desktop displays after their computer becomes reachable again, without restarting the desktop session.
- 079f75f: Hide obsolete desktop connection errors and the Reconnect control when the sandbox or its display stops. Keep the controls for starting the sandbox or recovering its display visible.
- 4c185d6: Stop unresponsive desktop tunnel processes after closing a viewer or an unexpected application exit.
- 1783cc3: Release desktop display connections when their viewer client disconnects, even if the guest keeps its connection open.
- 5730a4b: Improve error message and destructive button text contrast in both themes, including hover states.
- 1cd33df: Refuse Dev configuration imports through linked channel directories or files before copying any settings or credentials.
- 9b45d94: Flush imported Silo Dev settings and backups before continuing, and remove partial private temporary files when a write fails.
- d1c99f3: Keep private configuration and SSH key backups owner-only when importing settings into Silo Dev.
- 2c1aadf: Stop configuration imports if Silo Dev starts while the replacement confirmation is open.
- 7449845: Associate status captions with labelled disclosure buttons so assistive technology can report summaries such as failed dependency checks.
- 2b15594: Use MiB for binary disk-space amounts in update, VM image, and ChatGPT download errors.
- c74a033: Flush authorized-key directory changes before acknowledging remote access updates, including unchanged retries after a failed save.
- 606a6c6: Save SSH connection files to disk before confirming an editor configuration update.
- 9ae3517: Save repaired sandbox image information to disk before reporting success, and report errors if saving cannot be completed.
- 0bff67e: Save log exports to disk before confirming success.
- f545de5: Save remote-management settings to disk before confirming success.
- 6ae9210: Save port mappings to disk before confirming a configuration update.
- 972b40c: Save secret settings to disk before confirming success.
- 9f7dd52: Explain when a VM folder name contains unsupported control characters instead of opening a different folder in the editor.
- e9715d6: Linux editor desktop entries now preserve literal percent signs in executable paths and launch arguments.
- 81416ac: Open Linux editors whose desktop entries use `env -C` or `env --chdir`, retaining their chosen working directory.
- d4f8405: Open the exact requested VM folder in VS Code and Zed when its name contains literal percent escapes such as `%20` or `%2F`.
- c052979: OpenSSH checks now report unavailable client tools when installed files lack executable permissions.
- 106e60e: Preserve existing VS Code workspace configuration and explain how to repair it when Silo cannot read it, instead of silently replacing it.
- c156b30: Repair permissions on reused SSH connection keys without changing their identity, so editor and sandbox connections keep working after a key becomes broadly readable.
- fa20251: Show readable error messages in action and background notifications instead of “[object Object]”.
- 41d1913: Name the owning computer in log errors when sandboxes share a name.
- 8c59706: Show readable failure reasons when loading sandbox logs or viewing older entries, including connection errors.
- 96fa36a: Label ports as “Sandbox stopping” while their sandbox shuts down instead of saying it is starting.
- ba6f563: Show the failure reason and recovery guidance when a port change is rejected.
- 559b267: Keep complete recovery instructions and partial-change warnings visible when an error has separate diagnostic details.
- a54e6e0: Show port status as unknown when sandbox state is stale, including cached stopped, starting, and failed sandboxes.
- e2f1b29: Show a loading message in the tray during startup instead of claiming no sandboxes exist before inventory is read.
- c56158e: Keep unread checkpoint and reclaim information unknown after a storage read fails, and explain how to retry.
- 101f90a: Show the remote Silo update instruction without claiming logs are empty when that computer cannot serve logs.
- 8936209: Pressing Escape in the Commands palette no longer cancels a pending confirmation behind it.
- 646d68a: Keep Silo commands responsive while the export folder picker waits for backup status.
- ad8632a: Preparing a checkpoint export no longer changes your saved checkpoint history while another checkpoint operation is in progress.
- 98bfd3d: Settle pending export requests when Silo closes before the export starts reporting its progress.
- 6abbb98: Recover VMs left paused by failed full checkpoints. Resume them when possible, preserve their disks if a forced stop is needed, and keep recovery available through ordinary sandbox controls when recovery fails.
- 079344c: Return keyboard focus to the Add button when dismissing an import popover.
- 09c451a: Preserve keyboard focus when keeping an import or export running after its cancellation prompt.
- 023b4ef: Show the selected background and contrasting checkmark on checked checkboxes.
- e46d961: Announce confirmation and form popover titles and descriptions to screen readers.
- ba92a36: Keep filter selections and results unchanged while using keyboard keys to compose text with an input method editor.
- f3b74b9: Keep filter keyboard selection working when the available sandbox list changes.
- 32f1889: Keep the highlighted filter option visible when navigating long lists with the keyboard.
- 8eb03be: Preserve popover form drafts when Escape cancels an input method candidate.
- 4fcb8db: Keep GitHub repository selections and Git identity fields unchanged while confirming input method candidates.
- 5af7546: Keep inline confirmations open when Escape belongs to an input method composition.
- 72189d6: Announce the selected action when a menu or command-palette action opens a confirmation popover.
- 7a3e913: Keep focus on the clicked page control when dismissing a confirmation or form popover by clicking outside it.
- 5249d35: Restore scrollbar thickness and orientation styling in sandbox lists and scrollable panels.
- 3a2b3ac: Expose completed, running, pending, and failed operation steps to screen readers.
- d9b8fa9: Reject unsupported saved sandbox CPU counts, including values above 255.
- 7676c47: Clear an earlier backup read error after successfully deleting the pre-upgrade backup.
- cf9a38f: Ignore notifications delivered to an app view that has already closed.
- f4fdbc7: Show SSH configuration repair notices even when they arrive as Silo starts listening for them.
- 1ba94f1: Show a status read error and Refresh action when a computer returns an unreadable ChatGPT for Linux status.
- 7aeb46f: Show recovered export and import results even when they arrive as the migration notice opens.
- 8ddd134: Keep current ChatGPT for Linux status clear of errors from older status reads.
- 99a75d3: Acknowledge import and export results when their Open or reveal action closes the notification, so handled results do not reappear after restarting Silo.
- 888d62a: Remove stale Retry and Open buttons when a notification starts a new background operation.
- e9d13ff: Dismiss remote repository push notifications when their sandbox is deleted, while preserving notifications for same-named sandboxes on other computers.
- 8d0b35a: Keep operation progress and Cancel controls visible after retrying a failed background operation.
- c79591e: Keep sidebar hover previews open when keyboard focus enters from the sidebar toggle.
- 03b4072: Keep unrelated export and import notifications visible when deleting a sandbox after its notification has been replaced.
- 2b5da3b: Show soft hyphens explicitly in guest file, folder, and repository names so distinct paths cannot hide that difference.
- bc8813d: Keep unsaved sandbox edits when opening and dismissing the Add menu.
- 9471fc5: Pause sandbox settings saves while a checkpoint runs, keeping unsaved edits available when it finishes.
- 071b3f9: Recheck sandbox settings when confirming Stop and save, so newly reported computer limits show validation errors before saving.
- 6278da4: Preserve an open sandbox editor's protection against concurrent changes when deleting another sandbox.
- fa2a2ee: Recheck sandbox deletion eligibility at confirmation when configuration changes are locked or the owning computer becomes unavailable.
- aeb26b9: Disable sandbox reordering while editing to preserve unsaved drafts and their protection against concurrent changes.
- 0f81749: Keep a dismissed Stop and save confirmation closed after the sandbox stops or another change temporarily blocks saving.
- 7ad8738: Offer Review changes immediately when returning to a sandbox edit whose pending save was rejected while another page was open.
- db5100e: Keep a new sandbox draft open with a clear explanation if its selected computer is removed before saving, so another computer can be chosen.
- 21a8d80: Show the current resource value when switching a new sandbox to a computer with fewer available presets.
- 8c73cd2: Keep sandbox conflict and review notices with unsaved edits when navigating away and returning.
- 8012439: Keep sandbox resource presets within supported limits, even on high-capacity computers.
- f5b8179: Keep sandbox saves locked across navigation and clear saved drafts even when their editor is closed before saving finishes.
- 1eb4639: Keep local sandbox saves scoped to this computer when another computer is removed while an editor is open.
- 8e9de86: Pause sandbox settings changes when its status cannot be refreshed, preserving unsaved edits until Silo knows whether the sandbox is running.
- 7aa0fb9: Reduce repeated failed folder refreshes while healthy folders continue updating. Refresh visible folders immediately when you return to the window.
- f1176fc: Keep filter suggestions out of the Tab sequence so keyboard users can leave the filter directly.
- 58ca070: Restore the automatic GitHub CLI environment default when starting checkpoints, and retain supported environment defaults when importing exports.
- d66d6aa: Fix importing a sandbox checkpoint export: valid archives were refused with "its capture scope is not supported".
- db00e9e: The Linux desktop now starts reliably after a sandbox restarts or is imported: a leftover audio-server file from the previous session no longer stops it, a failed session start is retried, and computer use waits for the desktop instead of reporting that it was not running.
- 39131f5: Recover ChatGPT for Linux download progress after status event subscription fails, and offer Refresh while updates are unavailable.
- a3cb1a6: Report incomplete editor connection repairs when SSH configuration files cannot be read or replaced, and allow repair to succeed after storage becomes writable.
- 2b7b53e: Prevent GitHub settings from crashing when a newly discovered sandbox is named constructor.
- f7e252e: Allow turning off a sandbox's Git identity even when its name or email is empty.
- 38c15b9: Keep the highlighted GitHub repository stable when the catalog refreshes, so Enter cannot grant access to a different repository.
- 7cc26a0: Discard old GitHub edits and retry actions when a sandbox is deleted or replaced, so a new sandbox with the same name does not inherit them.
- 99cfa59: Keep Enable and Disable access available for connected personal tokens when GitHub OAuth is disconnected or connecting.
- fcaa567: Record scoped GitHub push credentials before use and retry their revocation independently of sandbox grant renewal, including after an app restart.
- 564433f: Start imported full checkpoints with a fresh boot of their saved disks, without resuming captured memory or processes.
- 7e15f73: Keep each sandbox's latest state when lifecycle responses arrive out of order on the same computer.
- cff4735: Require a desktop-entry file when choosing a browser on Linux, with a clear error for executable-only selections.
- ffbf104: Keep overlapping Linux notifications for the same operation from creating duplicates that remain after sandbox deletion.
- 31b210f: Preserve the correct sandbox notifications when the Linux desktop notification service restarts.
- bf253d5: Update the launch-at-login status when enabling notifications detects an external login-item change.
- 6cb10f5: Preserve completed and failed migration status when earlier Retry or status responses arrive late, and confirm status after Retry.
- 911c25b: Keep notification permission controls accurate when a settings refresh finishes before an older permission check.
- 20aa695: Preserve application preferences and current GitHub connection state when finishing setup after a sandbox deletion confirmation.
- 381c7f5: Prevent setup from crashing for a sandbox named constructor when GitHub policy or recovery fields are missing.
- f245c4d: Hide private-key blocks across retained log records, pages, searches, and exports.
- 775bea0: Prevent an old token-removal retry from deleting a replacement token or starting overlapping credential operations.
- f9bc0be: Keep the active GitHub personal token and account unchanged when saving a replacement fails.
- 8068dcd: Keep the recovered Git identity checkbox consistent with the identity settings submitted by setup.
- 174efa1: Make the initial Storage error notification's Retry read measurements again, while preventing overlapping requests.
- 694e322: Preserve pending GitHub token revocations when pushes and credential cleanup finish concurrently.
- b7ac419: Simplify the sandbox storage "reclaim unused space" block into a single "Unused space" row that shows the last result and a "Free up space" button, with matching wording in history and notifications.
- 823ce93: Keep action result notifications tied to the original sandbox so replacements cannot inherit old errors.
- f9f4363: Retry GitHub token checks and repository reads after an interrupted response body instead of leaving them blocked until an explicit retry.
- 16728d3: Saving a personal GitHub token now retries that token's checks without restarting failed OAuth token operations or checks for other credentials.
- ad18e24: Automatically retry safe GitHub token checks and repository reads after an HTTP request timeout instead of leaving them blocked until an explicit retry.
- 64afd70: Preserve GitHub's requested waiting period during service outages when retrying explicitly or restarting Silo.
- 4e792a5: Retrying GitHub access for one sandbox no longer restarts stopped token operations for other sandboxes or the account.
- 9229694: Fix sandbox GitHub access when GitHub omits the optional App client ID from an installation response.
- c328666: GitHub pushes recover on their own after a temporary credential-store failure instead of staying blocked until restart.
- 9df50d3: Bound memory use when rejecting an oversized GitHub configuration file.
- 028f603: Refreshing GitHub repositories no longer retries stopped token requests whose outcomes are unknown. Stopped requests for other credentials and workspaces, and GitHub's waiting periods, are preserved.
- 676130a: Handle interrupted GitHub sign-in responses without losing the authorization result.
- 9bf2577: Reject host pushes when GitHub access changes while their credentials are being acquired.
- 9f2a8c4: Preserve the last working GitHub configuration when a repository catalog exceeds the supported storage size.
- 21301b8: Keep the existing GitHub account and workspace access when saving a reconnected account fails.
- 027dba6: Keep selected GitHub repository access working when repository or owner capitalization changes.
- fe0ee59: Preserve pending GitHub access changes in other sandboxes when retrying one sandbox.
- 3fdee30: Keep sandbox state available when saved GitHub settings report no authentication method.
- 1e0e07c: Keep GitHub disconnect pending when token renewal fails because of an App configuration error, instead of forgetting credentials before revoking access.
- f97dc0a: Reject invalid GitHub settings revision numbers without changing saved sandbox choices.
- 999cfac: Discard newly issued GitHub tokens when Connect rejects an unsupported session, including Apps with token expiration disabled.
- 06e9d90: Preserve rejected GitHub repository and identity choices when retrying after settings refresh.
- b8e09e2: Use “1 second” in GitHub retry messages when one second remains.
- 9fad68c: Fix GitHub authorization and token management for source builds using a GitHub App client ID that contains a dot.
- 3e336a0: Expose invisible Unicode joiners, fillers, selectors, and tags in guest file and repository names while preserving original paths for actions.
- 867ffe4: Verify desktop packages before accepting a preinstalled guest desktop when its package list cannot be read.
- a248ed8: Keep computer-use tools able to discover all open Linux applications, even when checking other applications takes too long.
- bedcdfc: Limit sandbox desktop logs during streaming and retries so they cannot fill the sandbox's disk.
- 178b54f: Report permission denied when a workspace folder cannot be reached through its parent, instead of claiming the folder no longer exists.
- a9725cd: Preserve files outside the managed Linux desktop log directory when the home or VNC directory is a symbolic link.
- fb8bda4: Recover a failed Linux desktop session when computer-use setup retries Start, and reap exited desktop session processes.
- 3f74461: Stop starting additional desktop repair attempts after the computer-use setup wait expires.
- e7223a1: Explain checkpoint deletion blockers and that retained data can be released by deleting its last dependent checkpoint.
- 95b36ae: Update bundled help for Debian in-app updates, older remote computers, and ChatGPT download recovery.
- 6b505e8: Correct the macOS Settings menu path in the bundled help for Silo Dev.
- d8508ef: Clarify that in-app update instructions apply to production Silo and that Silo Dev has no update feed.
- d583084: Clarify that copying diagnostic details is available only where a copy control is offered.
- efc28d5: Explain that duplicated sandboxes with built-in computer use start their desktop automatically, even when the original used manual startup.
- e6d100e: Explain how to update a legacy desktop when its viewer requires an update before starting.
- 6f5ca5a: Clarify that reclaiming workspace storage frees allocated space on the computer while preserving workspace capacity and files.
- e8d82a8: Clarify that computer-use setup needs both the sandbox and its Linux desktop running.
- b1e072e: Update troubleshooting help to follow the System issue recovery instructions and rerun checks with Retry checks.
- b67c6b3: Keep search text out of log exports so searching for a credential or private text does not include it in a shared file.
- d2e5703: Preserve files outside the new home during account migration by stopping on conflicting files, folders, or links and reporting the path to resolve before retrying.
- d69e107: Keep pushes to different repositories independent when a failed push removes its publishing cache.
- e2bbc59: Refresh repository state after a push even when an older discovery is still running, instead of restoring stale commit counts.
- 945a372: Allow repository pushes to retry when saving the initial push record fails, instead of leaving an unstarted push stuck in progress.
- ab90928: Report a GitHub push with a lost acknowledgment as unknown, while keeping explicit branch rejections actionable.
- 36032dd: Report an interrupted GitHub push as unknown when host disk-space or process-status checks stop it, so you can check the branch before retrying.
- 9214ed8: Preserve newer fetched tracking data and repointed origins when a sandbox push finishes.
- 68d4623: Prevent push completion from changing a local branch through a symbolic remote tracking ref.
- 364d4f0: Require acknowledgment of an unknown push result before another computer can start a new push for the same repository.
- b015a5d: Focus the available Cancel or retry action when an import is being checked or cannot be imported.
- 4253d23: Keep the import name field's label stable and associate duplicate-name and format errors with the field for assistive technology.
- 074d712: Show each sandbox’s completed log read immediately while other computers are still loading, retaining previous results during refresh.
- c4b915f: Preserve concurrent port saves across computers and sandboxes, and confirm current settings after an older save reply arrives.
- 7669cf5: Preserve saved GitHub access and Git identities when keeping sandboxes omitted from recovered setup.
- efeb68d: Computer use now works for all supported agents on Linux desktops; previously only some agent modes could reach the display.
- 4e9ff7b: Agents can now type, click and scroll in GTK 4 apps such as GNOME Text Editor on the Linux desktop.
- a52358b: Computer use on the Linux desktop is safer: agents' keys and clicks no longer risk landing in the wrong window when actions overlap, and held keys are always released.
- e6eb6b3: Computer use on the Linux desktop only redirects an agent's input when Silo can confirm which app owns the window, and held keys behave as in Codex.
- 99ee657: Computer use on the Linux desktop refuses a click when another window covers the target, and recovers on its own if the computer-use engine stops responding.
- a66babb: Computer use on the Linux desktop never leaves a mouse button or key pressed after an interrupted action, and refuses clicks while a popup holds the pointer.
- 1adc67e: Computer use now uses LCU 0.8.8, which registers every supported agent, including ones installed after the sandbox was set up.
- ac01274: Show the current sandbox operation step when its delayed progress notification appears.
- 71de672: Clear sandbox operation progress notifications when they are disabled or their view closes, and restore them when tracking resumes.
- a201c98: Prevent an older Start, Stop, or Restart retry from undoing a newer lifecycle action on the same sandbox. Local and remote actions share this ordering; actions on other sandboxes still run independently.
- 302575e: Keep Linux system updates from accepting an incomplete installation confirmation after its timeout.
- ada1f3b: Keep sandbox links on their owning computer and prevent recreated sandboxes from inheriting old navigation history.
- 093b574: Hide command lines with credential options in sandbox logs, failure details, and exported diagnostics.
- 360ae71: Restore keyboard focus after closing the log date filter editor.
- b335fa3: Make expanded diagnostic and setup activity output focusable so keyboard users can scroll it.
- 5aff97c: Check for log export cancellation after writing and syncing, before replacing the destination file.
- 94369b7: Treat a cancelled log export as cancelled when its pending log query also fails.
- d2e5048: Cancel log exports that have not started yet without opening a save dialog or replacing an existing export.
- 4cfb811: Include the year in log date filter summaries so ranges across different years stay distinct.
- 282d289: Make expanded log messages focusable and named so keyboard users can scroll long output.
- a569666: Count unfinished private-key redaction state against the log search memory budget, so searches over many such records fail with a narrow-your-search message instead of using unbounded memory.
- e4bd3f9: Keep log age tracking and cleanup working when Linux storage folders have unusual names.
- dc07d15: Use singular labels for one matching log record and one storage reclaim attempt.
- 948c21c: Hide credential-bearing URLs in sandbox logs, failure details, and exported diagnostics.
- b81e274: Hide signed URLs and URLs with encoded credential query parameters in sandbox logs and exported diagnostics.
- 0733410: Report an error when the macOS browser launcher stalls instead of waiting indefinitely.
- c3527dd: Keep navigation requests available to the active main window after an older view closes.
- f52f5af: Preserve the latest requested destination in the main window when earlier navigation finishes late.
- d4c2343: Retry opening the installer page when View installers on GitHub fails.
- 2d70389: Update the bundled MicroSandbox runtime to 0.7.6. Commands that run inside a sandbox (identity checks, tool verification, repository and account setup) no longer wait on an open input pipe, which could leave an operation hanging. Checkpoint exports made with the previous runtime still import.
- 4020e7c: Move links to legacy home directories into the working account's home while preserving links to external and relative paths.
- b274d79: Prevent completed storage migrations from reopening pre-upgrade sandbox data when the saved storage selection is missing.
- 2f27354: Migrate shell and launcher settings that name an entire legacy home directory while preserving external paths with similar names.
- 0d3ec46: Keep the app responsive while migration progress waits for a storage write.
- e7b5293: Silo now lives at github.com/amontlabs/silo and silo.amontlabs.com, and Debian installs get updates from apt.silo.amontlabs.com. Existing Debian installs must run `sudo sed -i 's#https://0xpolarzero.github.io/silo/apt#https://apt.silo.amontlabs.com/apt#' /etc/apt/sources.list.d/silo.sources` once to keep receiving updates.
  
  The Silo GitHub App is now `silo-amont-labs`, owned by Amont Labs. Update Silo to keep using GitHub repository access; earlier versions look for the app under its previous name.
- 10fd55f: Refresh expired folder listings when a sandbox is replaced by another with the same name.
- 43737a9: Allow log export to be retried after an unexpected export-task failure.
- 35d0a37: Preserve the previous log export and request an update when a selected remote computer cannot serve logs.
- c4de1e2: Avoid invoking the macOS developer-tools installer when a Git shortcut points to Apple's Git shim and developer tools are absent.
- e195931: Ignore non-executable files when identifying another Silo launch through PATH.
- 18004c1: Report SSH connection key cleanup failures when removing a computer, and keep the computer listed so removal can be retried.
- a8d074b: Keep Linux tray health updates that arrive while the status icon is starting.
- fee1755: Preserve the instruction to start a sandbox when it stops during a file listing.
- 5551cca: Close native sandbox menus when their controls disappear and ignore late menu creation or obsolete actions.
- b3a21a6: Keep remote network services loading until that computer responds, and retain cached sandbox ports while waiting.
- 846f4af: Remove saved port forwards when deleting a sandbox so a new sandbox with the same name does not inherit them.
- 31c5ca3: Show “No matching sandboxes” immediately on the Network page when a filter matches no sandboxes.
- ca49747: Name the sandbox and owning computer in port-operation feedback and link remote system notifications to the correct sandbox.
- b8322d1: Prevent a port failure’s Retry action from starting a second save while another port change is in progress.
- cb1bc34: Keep failed port removals visible and retry them on refresh, including when a sandbox has no other published ports.
- 28186b4: Apply the existing saved-port settings limit while reading the file, preventing oversized inputs from allocating their full contents before rejection.
- 3fec180: Stop refreshing editor and migration notifications after their views close, and ignore outdated backup updates.
- f0e12e3: Show failed setup draft saves during onboarding and let you retry without losing your edits.
- e4d2ec9: Keep a chosen GitHub OAuth sign-in when you change a sandbox's repository access during setup and resume later.
- 6ddaab8: Preserve existing GitHub access when sandbox policies load during onboarding, without replacing edited or recovered choices.
- 6c4f1f6: Use singular sandbox labels when reviewing setup for one sandbox.
- 2c0c883: Keep each sandbox's GitHub authentication method through onboarding and draft recovery, and enable token selection when a token is connected.
- 9c03183: Open the Linux desktop from an icon next to Open in terminal and Open in editor.
- 2348531: Let Escape dismiss an operation cancellation question and return to its Cancel button without stopping the operation.
- 1cc511e: Keep command confirmations tied to the current sandbox, and cancel them when that action becomes unavailable.
- 6fdb721: Focus the personal-token field when editing opens and return focus to Add or Replace token when editing closes.
- 273dda6: Keep port drafts tied to their original sandbox so a replacement with the same name cannot receive an older edit.
- 655ddab: Keep browser-open failure notifications separate for ports on different sandboxes and computers.
- 7b80245: Prevent old port-operation retries from targeting renamed or replacement sandboxes or running after their Ports view closes.
- 4cf3d00: Preserve edited or new port drafts when an older failed save succeeds through its notification Retry.
- 945ae66: Show failed preference saves in General settings and offer a retry while retaining edits. Explain when write-protected settings last only for the current session.
- 91c8f4f: Preserve binary launcher files when migrating older sandbox accounts, even when they contain the old home folder path.
- b9895ce: Keep temporary export and restore files private to the current user.
- f27c01c: Create new workspace disk directories with owner-only permissions to protect private files stored inside VM disks.
- b0d3413: Prevent late background events from updating an app view that has already closed during startup.
- 14d2ed8: Only offer push cancellation for a queue entry owned by the current sandbox, including after a sandbox is recreated.
- 124e0ff: Update push notifications when progress messages, failure details, or confirmed retry targets change.
- e4153c1: Keep Silo commands responsive while preparing a repository push.
- 7a49914: Show a persistent warning when a repository push's outcome cannot be confirmed. Closing the warning still requires checking GitHub before another push.
- 4f22309: Allow computer-use setup to repair invalid saved setup status instead of failing to load it.
- dea237a: Preserve shell configuration encoding and line endings when migrating older VM accounts.
- c861294: Keep healthy sandbox ports reachable when a same-named sandbox on another computer fails discovery, and identify the affected computer in ambiguous error messages.
- 5a8012c: Avoid flashing progress notifications for quick operations that replace completed work between queue updates.
- ab9bc40: Restore the previous keyboard focus after dismissing the main window's Quit confirmation.
- e375abe: Improve pending operation step text contrast in light and dark themes.
- d78a22f: Include the year in storage reclaim history so older attempts remain distinguishable.
- f02b272: Verify recovered setup correctly for a sandbox named constructor with no saved Git identity.
- ef10a69: Prevent recreated sandboxes from inheriting pending actions or late lifecycle results from their predecessors.
- 027c6b5: Keep script launchers usable when migrating older sandbox accounts, preserving their text format and executable permissions.
- 6bd7cc6: Do not start remote changes after their timeout expires or management permission is revoked, even if checking the connection took too long.
- b6d58e0: Treat missing or malformed computer-use approval status from remote computers as unknown, while preserving warnings when the chosen and applied approval modes differ.
- 3627d83: Report a timeout when the receiving Silo instance stops accepting a remote request.
- f68b8f0: Give remote checkpoint actions their full supported timeout, including time spent waiting and connecting, and keep retries tied to the same action.
- 22ab074: Reduce repeated background reads when the saved computer list is unavailable, while keeping local status and manual refreshes responsive.
- 1c7f2fc: Reduce repeated status requests to offline computers while keeping healthy computers up to date and manual refreshes immediate.
- bfe869a: Preserve remote bridge links to unrelated AppImages instead of replacing them when remote management is enabled.
- 8de109a: Prevent malformed remote operation records from blocking remote changes or being treated as unrecorded operations.
- 20d3b84: Preserve SSH key restrictions when multiple computers connect at the same time.
- 9fe3a1f: Accept remote replies up to the supported size even when the SSH login shell prints a banner.
- e87d3c6: Keep remote requests within their timeout when SSH stops accepting input, and reject oversized requests before connecting.
- 8da432f: Reject special remote settings files promptly so they cannot freeze remote-management settings and authorization reads.
- 74378bc: Stop queued remote SSH key authorization after access is revoked, and record key changes so connection retries do not repeat them.
- 4a195f4: Reduce repeated ChatGPT download status checks when a remote computer is unreachable, and resume normal checks after it reconnects.
- dbf4a2c: Apply remote IPC read deadlines to complete messages so partial traffic cannot keep an incomplete request or reply alive indefinitely.
- bc122bf: Keep remote guest-access preparation bound to the selected sandbox while it waits. Renames use the current name, and a replacement sandbox cannot receive the original request.
- 635f1d4: Repair existing SSH directory and authorization-file permissions when installing Silo's remote-management key, so repeated setup can restore key authentication.
- fc01433: Report a repair error when an older remote management key cannot be restricted during connection setup.
- 8b94f90: Preserve other sandboxes' latest state when a remote sandbox is added, edited, or deleted.
- 1abe87e: Keep the latest remote-management choice when an older status request finishes or fails.
- 7902096: Reserve remote connection capacity during saves and while disconnected ports await reconnection.
- d8e8466: Keep remote port connections and reconnection attempts intact when a computer returns invalid network information.
- 2a338bd: Preserve remote port connections when a computer temporarily cannot inspect a sandbox's network state.
- 0e96253: Preserve newly saved remote port connections when an older network refresh finishes.
- 213434b: Prevent an older remote network refresh from replacing endpoints or closing connections verified by a newer refresh.
- ab04227: Discard remote reconnect attempts superseded by cleanup, a new connection, or an endpoint change.
- 7a80db8: Keep removed remote ports disconnected when an earlier connection attempt finishes.
- 7565a8c: Reject outdated network information when a remote sandbox is replaced during a status check.
- 7d1c9c3: Open remote sandbox notifications on the correct computer when sandbox names match, and preserve notification links after renaming a sandbox.
- 5750e6d: Treat remote actions with oversized saved recovery records as uncertain and avoid repeating them after restart, without using excessive memory.
- 2a13890: Close a computer's local access to a remote port once the other computer removes its publication.
- 2642f87: Report malformed remote replies as connection errors instead of treating missing results as successful actions.
- 76a08eb: Accept remote replies after permitted shell startup output containing partial reply markers, while preserving the output size limit.
- 3740bf1: Stop reconnecting for remote changes after their request deadline expires.
- 72780be: Stop retrying remote actions after Quit begins, including when shutdown fails and Silo stays open.
- ce55af9: Repair reused remote management key permissions and derive the public identity from the private key when setting up or upgrading a connection.
- 90885fc: Keep Network and SSH status fresh for healthy computers while another computer is slow or unavailable.
- 1b07e21: Keep the app responsive while loading or saving remote computer settings.
- 6081120: Reject remote-management settings larger than 1 MiB without replacing the saved settings, preventing oversized files from exhausting memory during remote status reads.
- e2de3fe: Keep remote snapshots, port changes, and VM actions working while many port, desktop, or editor connections to another computer are open.
- 0a76489: Close published remote-port SSH tunnels and their child processes when the controlling Silo app crashes or exits.
- 33c1223: Confirm remote port forwarding is ready before showing its local address, so an unrelated local service cannot be mistaken for the connection.
- 91ff5be: Retrying a failed sandbox creation no longer fails after the built-in desktop default, repairing an incomplete preinstalled desktop restores the full set of packages and settings, and creating a sandbox on another computer allows time for its desktop.
- a2f1ef0: Restore update notifications when you return to Silo after their connection failed, and keep connection errors visible until automatic updates work again.
- 0ec9bd8: Prevent recreated sandboxes from receiving cached repository listings or commit counts from their predecessors.
- dd5617a: Report a repository discovery error when Git cannot inspect working tree changes instead of showing the repository as clean.
- 0f070d8: Keep the highlighted repository or authorization action visible when navigating long repository lists with arrow keys.
- fc4423a: Keep GitHub repository suggestions out of the page Tab order while preserving arrow-key and Enter selection.
- dd2b6d8: Fix computer-use setup and storage reclaim after the MicroSandbox 0.7.6 update. The bundled runtime stopped reporting which run of a computer is active, so a built-in computer was never set up for computer use after it started and its disk space was not reclaimed after a start. The runtime reports it again, and Silo now says so plainly if a runtime ever lacks it.
- fce3007: Retry restoring a theme saved by an older Silo version after a temporary settings failure, without requiring an app restart.
- 93e042b: Retry update installation after settings delivery or other preparation fails without discarding the selected update and checking for another release.
- dedb628: Show complete text on hover for truncated row details, disclosure headings and captions, and status badges.
- 96a29d8: Show complete file, folder, and sandbox names on hover in the file tree and menu bar folder picker.
- 87a5331: Show complete selected filter names on hover when their chips truncate the text.
- ff53a55: Show complete operation checklist step labels on hover when notification text is truncated.
- 7c68425: Keep log entries visible after an unusually long line instead of skipping the following entry.
- 8cf92a0: Reject oversized saved sandbox actions without using excessive memory or waiting indefinitely.
- 1edeee1: Record a failed sandbox action in Activity when its recovery progress cannot be saved, instead of leaving it marked as running.
- a71eadf: Show placeholders for damaged execution log entries with missing or invalid messages.
- 144696e: Show unfinished actions from a previous app launch as interrupted even when the operating system reuses Silo's process ID.
- 4ac091a: Preserve replacement sandboxes when saving or verifying Git identities, including replacements made while an identity update waits to run.
- 55573f4: Prevent pending GitHub access updates from sending credentials to a replacement sandbox with the same name.
- 1ce27d9: Preserve replacement sandboxes when saved settings refer to an older sandbox, instead of stopping or reconfiguring the replacement during an edit.
- a8b17f2: Reject secret updates when the selected sandbox was replaced before the update could run.
- 6fa2ca4: Preserve a checkpoint fork's recovery state and assignments when its saved sandbox list cannot finish syncing to disk.
- bbdb841: Keep pre-upgrade backups while sandbox images still depend on their files, or when those dependencies cannot be checked.
- e5cc5ff: Keep timeout and connection checks active for queued remote actions.
- 7fbf778: Dismiss sandbox crashes successfully when activity history is damaged, while preserving that history and showing a warning.
- bf1637f: Preserve oversized sandbox activity files and show a warning instead of replacing them after reading only a valid prefix.
- fbbef3e: Report sandbox images that could not be checked or repaired, even when other image repairs succeed.
- b414c42: Report missing sandbox image files during repair, including files already expected in the current storage location.
- 6bb7e75: Allow already-started remote actions to finish after their original start deadline expires.
- ae83d9f: Keep pre-upgrade backups when sandbox image information is redirected or too large to check safely.
- 03ef714: Report a startup error when a running sandbox has replaced the one you selected, instead of treating its reused name as a successful launch.
- 436a541: Keep secret revocation pending when a failed checkpoint restore leaves a running sandbox with access. Read that sandbox's actual state for terminal, network, and file access.
- 30f459a: Avoid reporting successful storage reclamation as a failure when a sandbox writes to a checkpoint disk at the same time.
- d187f6a: Allow Quit to stop a replacement VM when an unfinished configuration change reuses a removed VM's name.
- d5c08ac: Allow Quit to complete when an unfinished sandbox setup's Stop command reports an error but the exact VM is verified stopped. Keep shutdown blocked if it is still running or cannot be verified.
- d5b8009: Show the actual disk usage and maintenance errors when a failed checkpoint restore has already created a sandbox. Explain that its restore must finish before reclaiming space.
- 2c213d2: Block updates when an unfinished sandbox setup still has a virtual machine, so installation cannot overlook it.
- 1df85c2: Reduce repeated sandbox health checks while their virtual-machine service is unavailable and resume normal checks after recovery.
- 4f3fd1c: Preserve converted sandbox storage when retrying a migration that stopped after selecting the upgraded runtime.
- 9c3b7dd: Keep Silo's managed virtual-machine folders private to the current user and repair existing folders with broader permissions.
- af57838: Correct Safari website-address guidance for macOS 14 and 15, and explain the cookie-sharing limitation of the 127.0.0.1 fallback.
- bbf9c9f: Reject oversized saved sandbox configuration before reading the entire file into memory, keeping the existing 1 MiB limit and preserving the saved file.
- c11d30e: Show port discovery failures without a misleading “No ports” result on sandbox detail pages.
- 8b5dbf2: Return focus to Add after cancelling sandbox import before a review opens.
- a0665d7: Support standard keyboard navigation in the Add sandbox menu and preserve focus when opening an editor.
- 3ce9ba6: Expose computer badges and read-only disk values with accessible roles and names, and show keyboard focus on read-only disk groups.
- bff211d: Name sandbox deletion confirmations for screen readers and describe the sandbox and data affected by each destructive choice.
- 3a59a16: Focus sandbox editors immediately for direct edits while preserving focus after menu selections.
- 0f57a3a: Keep keyboard focus in the sandbox editor when duplicating settings from the row menu.
- 4601883: Return keyboard focus to the sandbox row or Add button after closing an inline sandbox editor.
- 474abc5: Expose sandbox lists and row controls as named groups for assistive technology.
- 36318c6: Describe the arrow-key controls for reordering sandbox rows to screen readers.
- 3ea003d: Announce pending sandbox settings saves to screen readers.
- 2e620d9: Require reloading changed secret access settings before saving an older draft, while preserving its replacement value.
- 855fd15: Keep removed secrets out of the list when an older save reply arrives late.
- d720085: Improve the light-theme contrast of secret restart notices.
- e04ccfb: Explain how to recover from secret storage capacity failures or a sandbox deleted during a save, while retaining the draft.
- db0a2b0: Keep concurrent secret saves from restoring assignments to a deleted sandbox or granting them to a new sandbox with the same name. Let sandbox cleanup finish while an update is being prepared.
- a56c3d4: Reject oversized secret settings before replacing the saved document, so existing secrets remain readable and editable.
- 9ddab17: Show secret assignments on the local sandbox when a remote computer has a sandbox with the same name.
- 80b89aa: Explain secret value, sandbox, and domain limits in the editor before saving.
- 16382b5: Keep Silo commands responsive while reading secret settings from slow storage.
- 7a46078: Preserve newer secret changes and their error status when an older runtime update finishes after concurrent edits.
- 80c2967: Offer Retry when concurrent secret edits exhaust live application attempts instead of leaving an indefinite Applying status.
- 9eec0c6: Preserve the saved computer-use approval choice when switching from a Silo version that records an unfamiliar setup outcome, and retry setup with that choice.
- c168221: Keep the saved export folder when another app version adds preference fields, and preserve those fields when choosing a new folder.
- 0cd7199: Preserve preferences added by another Silo version when saving GitHub settings.
- 6c8daee: Keep Storage history readable after switching Silo versions when saved maintenance records contain additional fields, and preserve those fields on save.
- de18755: Preserve preferences added by another Silo version when saving remote-management settings.
- 7bc32c9: Keep the automatic-update-check preference readable after switching Silo versions when additional preferences are present, and preserve those preferences on save.
- a8950c1: Avoid immediate network and SSH retries when a background timer fires during a slow status read.
- 27a99d1: Reduce repeated network and SSH status requests after a computer fails to respond, while healthy computers and manual refreshes stay responsive.
- 565ed23: Preserve onboarding recovery after deleting the last sandbox, including an unfinished replacement sandbox.
- 649e28f: Avoid reporting a settings-save failure during Quit when a newer settings event supersedes the final status read.
- 25f9af0: Prevent a pending sandbox-status read from reopening the Quit confirmation during logout or system shutdown.
- c40e139: Save the latest setup draft when delivery is slow or interrupted instead of replaying superseded edits before completion.
- 01ae31e: Finish sandbox shutdown within short Linux logout and shutdown deadlines, leaving time to release the system's wait for Silo.
- 7ca8e20: Honor macOS logout and shutdown requests while an earlier Quit confirmation or settings flush is still pending.
- 111f8e9: Protect saved onboarding recovery from overwrites when a sandbox's desktop policy is missing required data.
- 64a3057: Honor logout and shutdown deadlines when Quit is waiting for local sandboxes or settings to save. Keep sandbox starts blocked if that Quit fails during shutdown.
- 6c6a37a: Honor short logout and shutdown deadlines even if settings save confirmation does not arrive.
- 4e770a5: Keep successful settings synchronization visible when an older settings read later fails.
- d5760fa: Keep sandbox lifecycle and repository push notifications up to date when switching between Sandboxes and Files.
- cccddd0: Skip applying a blank Git identity during setup when no host author is configured, while still prefilling an identity that loads later.
- 6de401d: Show this computer's loaded sandboxes after cancelling a setup editor opened before they loaded.
- 4c8d213: Calculate setup elapsed time correctly when a reported start timestamp is zero.
- 2f390b9: Stop GitHub setup status checks when their app view closes.
- 17d1610: Show personal-token access separately from OAuth repository restrictions in the setup review.
- bf39bb5: Apply personal-token GitHub settings during setup even when OAuth is disconnected, and show token access in the review.
- dadbeba: Keep the sidebar preview open when another control handles Escape or an input method is composing text.
- 387c7fa: Show small export file sizes in bytes or KiB instead of rounding them to zero MiB.
- 2ec2c2c: Show empty and small disk allocations accurately in storage summaries and sandbox deletion confirmations.
- cd37227: Revoke exported SSH keys when access is disabled even if their local key files are missing. Report a recovery error for older settings whose managed key identity cannot be recovered.
- 3fd76cb: Skip ports already occupied on the selected network address when enabling SSH access for the first time.
- 7842530: Preserve newly registered SSH keys when editing ports, addresses, or access toggles from an older UI snapshot. Explicit key-list updates still replace authorization.
- 53b71a4: Preserve manually authorized SSH keys when a connecting computer registers the same public key, including after access is disabled and re-enabled.
- 96f129b: Keep remote SSH key registration safe to retry, and prevent abandoned requests from changing authorized keys.
- 8a09515: The sandbox page's **SSH access** tab is now called **SSH**. Clicking a sandbox's SSH badge, in the sandbox list or on its page, opens that tab, and the badge on the sandbox page now lines up with the status text beside it.
- d511ecf: Prevent SSH failure retries from changing access while another SSH save or connection preparation is in progress.
- 3bd88d0: Stop stalled SSH public-key extraction after five seconds so editor and desktop connection setup can recover.
- 8b2abdf: Ignore obsolete SSH Retry actions after newer operations or closed sandbox controls, and respect current read-only state.
- 7b243ef: Reset SSH controls when a sandbox is replaced so old retries and late connection commands cannot carry into its replacement.
- 099194f: Keep independent SSH settings changes for different sandboxes when save responses arrive out of order.
- e7994e4: Starting a sandbox now shows its progress right away, and the computer-use check that follows a start runs silently instead of appearing as a separate step.
- 1bb25a0: Startup checks no longer report the runtime unavailable when the computer is busy.
- d7f7da1: Pressing Escape cancels the status panel’s quit confirmation before dismissing the panel.
- 7cf496a: Cancel queued folder reads when you close the status panel's folder picker.
- 8e6fd11: Reduce repeated folder reads after failures in the status menu, while keeping focus refreshes immediate.
- c8b822a: Do not open a sandbox actions menu if its status row disappears while the native menu is being created.
- 4b6b075: Handle tray panel startup failures and ignore updates after the panel closes.
- b2b22a5: Keep the development status panel's current height when an earlier resize request fails.
- d71eee2: Show storage reclamation results even when you leave the Storage tab before the operation finishes.
- 2e0c641: Use less memory when building Silo from source, and preserve cached files if a download is too large or stalls.
- f7e8036: Keep expired Quit requests from saving settings after shutdown has already rejected the request.
- f19d564: Restore in-app notifications when their connection initially fails and you return to the Silo window.
- c4f9f6a: Recover the Quit overlay when the shutdown event connection initially fails and the Silo window regains focus.
- b95c627: Keep running work and its cancel control visible during Quit when queue event registration fails.
- 41909dd: Show current running work and cancellation controls when Quit starts while background updates are still connecting.
- d2314eb: Recover the Quit overlay on window focus when its initial shutdown-state read fails.
- 698d661: Keep onboarding recovery drafts private to the main window and preserve them when shared settings change.
- ef49053: Show an error before opening a terminal when required storage or library folders have unsupported names, instead of using a different path.
- e6db05b: Notifications now keep their icon, title, action and close button on one centred line, with any extra content below. The "Created" notification shows the "Allow without asking" switch for every new sandbox with built-in computer use, even before its first start.
- 7beae5f: Ignore malformed sandbox setup status from a computer instead of crashing the sandbox list or status panel.
- 4e153b0: Disable connection removal when the current integration does not provide that action.
- be13d52: Report that sandbox configuration needs refreshing when saving an empty setup before its state has loaded.
- 911645a: Ignore invalid optional startup and application preferences received from a computer so its other settings remain usable.
- 2c6a6c9: Preserve unfamiliar sandbox configuration fields and show readable field names when reviewing concurrent edits.
- 690de7e: Show configuration validation errors in the sandbox editor when they do not belong to an editable field.
- 1f2a562: Keep storage history usable when a saved reclaim trigger has an unknown name.
- 35f3934: Avoid redrawing the computer-use panel when periodic sandbox status reads return unchanged data.
- 8e9bd4c: Avoid redrawing computer-use download progress when its status or connection errors have not changed.
- e04d084: Avoid unnecessary interface refreshes when background checks find no changes.
- 0a2f64a: Avoid redrawing update status when background checks or repeated notifications report no changes.
- b1d0652: Removing a published port now immediately cuts off other computers still using it.
- 330a974: Retry downloading an available update after a command delivery failure instead of starting another update check.
- 1088e6c: Reduce repeated update installation checks when the updater cannot answer, and resume normal checks after recovery.
- db129eb: Keep pending preferences and onboarding drafts when saving fails, and require a successful save before installing an update.
- 7a8938a: Keep the latest update status visible when an earlier status check fails.
- f051295: Preserve Debian update error details even when the saved log is incomplete or contains invalid text.
- 8ff76c3: Reject oversized update recovery files without loading their complete contents into memory.
- 68f17fd: Preserve sandbox update recovery when the saved sandbox configuration is missing.
- c9333cf: Handle oversized update preference files without loading their complete contents into memory.
- 6082fb6: Keep automatic update preferences consistent when changes are saved at the same time.
- a0af439: Keep the Debian update preparation timeout active when the helper stops reporting progress.
- 366470f: Clear the update preference read error after successfully saving a replacement preference.
- a108f62: Updating Silo no longer fails to relaunch when an unfinished sandbox creation from an earlier version was waiting to be resumed.
- 864fef3: Require a fresh update request after the selected release or installation eligibility changes, instead of restoring a withdrawn sandbox-stop confirmation.
- 71bf39b: Expose sandbox badges as named groups so assistive technology can identify their sandbox state and computer.
- 071392a: Keep long names and paths inside tooltips, including tooltips with keyboard shortcuts.

## 0.10.0

### Highlights

- **New VM runtime and one-time migration.** Silo now bundles MicroSandbox 0.7.4. The first launch converts existing sandboxes to the new storage and keeps the previous storage as a pre-upgrade backup for 14 days (Settings → General → Storage shows it and can delete it sooner). Sandboxes from older versions move to the `silo` account on their own the next time they start.
- **Checkpoints.** Take manual checkpoints, fork a sandbox into a stopped copy you start yourself, restore with an automatic recovery checkpoint, and delete checkpoints you no longer need. Each one shows its size.
- **A page for each sandbox.** Click a sandbox to open its Overview, Checkpoints, Storage and Access tabs. Edit, delete, and manage secrets and ports from there. The sidebar and title bar have a refreshed frosted look.
- **Actions queue up instead of failing.** When another operation is running, an action waits its turn and shows what it is waiting for. Queued operations, and some running ones, can be cancelled. Confirmations open next to the button you used, and results come through notifications that stay until you dismiss them.
- **Export and import.** These replace the Backup tab: **Export…** saves a sandbox or a single checkpoint to a `.silo-backup` file, and **Import…** in the sandbox list brings it back. Both run as background notifications.
- **Remote computers.** Changes to different sandboxes on another computer run in parallel. SSH to a remote sandbox uses a key that belongs to the connecting computer, and turning SSH access off revokes it.
- **Safer handoffs.** Push asks you to confirm the repository, branch and commit before sending. Each published website opens at its own `*.localhost` address so its cookies stay separate. VS Code opens sandboxes in a separate "Silo" profile.
- **Quit** asks before stopping running sandboxes and names them.

### Before you upgrade

- Exports made with Silo 0.9.0 or earlier can't be imported into 0.10.0. To move a sandbox to another computer, export it again after upgrading.
- Remote management between two computers requires 0.10.0 on both.
- On Linux the pre-upgrade backup can be as large as all your sandboxes. Once you have checked your sandboxes, you can free that space from Settings → General → Storage.

### Minor Changes

- a118707: Sandboxes from older Silo versions now move to the silo account by themselves the next time they start, instead of refusing to start. Their home files are copied into the new account and the originals stay in place. Checkpoints and exports taken before the move are set up the same way when they first start.
- 2762e1c: Push now asks for confirmation naming the GitHub repository, branch and commit before anything is sent, and publishes exactly that commit to that branch. If the sandbox's origin, branch or commit changes after you confirm, the push stops with "The repository changed after you confirmed the push" instead of publishing something else. Retrying a failed push from its row confirms the repository's current state again. Repositories without a GitHub origin can no longer start a push. Pushing to a sandbox on another computer requires Silo on both computers to include this change.
- ab70269: Background actions such as GitHub settings, pushes, checkpoints, storage reclaim, ports, log export, and sandbox start or stop failures now report through notifications that stay until you dismiss them, with Retry on failures. The Network page disables adding ports while the sandbox is stopped and names the forward action "Forward to this Mac".
- be381f0: Queued operations can now be cancelled, and some running operations (starting or restarting a VM, capturing a checkpoint, backing up, setting up guest tools or the desktop, and applying GitHub access or secrets) can be cancelled while they run. Operations that run longer than expected are flagged as "Taking longer than expected", and safe actions like starting, stopping, or restarting a VM retry automatically after a temporary failure. Failed lifecycle actions offer a Retry that re-runs the same intent against fresh state.
- 36ed3de: Use the saved application preferences instead of runtime placeholders, exclude legacy SSH entries from startup choices, and show their state as unavailable with guidance to connect the computer.
- d0eafeb: Checkpoints can now be deleted from a sandbox's Checkpoints tab, after a confirmation. Each checkpoint shows its size, and one that a fork, a later checkpoint or the sandbox itself still builds on shows what uses it and why it can't be deleted yet. Deleting a sandbox now also removes the checkpoint data only it used, a failed checkpoint no longer leaves its data behind, and Silo clears checkpoint data no sandbox uses any more. The Storage tab shows how much space checkpoints take.
- 6872e33: Keep desktop sessions running when the viewer disconnects, recover the display without restarting graphical applications, and let users explicitly update a stopped legacy desktop. Show LCU readiness separately from Luda and allow explicit setup against a running guest session without starting or reconnecting its display stream.
- b78710e: Edit and delete a sandbox directly from its detail page. Choosing Edit (the Resources row button or the ⋯ menu) now opens the sandbox editor in place — below the header, with the tabs hidden — instead of returning to the list, and saving keeps you on the same sandbox. Deleting a sandbox asks for confirmation in a dialog on the page and returns to the list once it is removed. The editor behaves exactly like the one in the list, including the stale-edit conflict review.
  
  Add "View all" links to the Overview tab: Repositories jumps to Files, Ports jumps to Network (both filtered to the sandbox), and Secrets opens the Secrets tab.
- 2cfdcc8: Export and import now run as background notifications instead of inline panels. Starting an export from a sandbox, its detail page, or a checkpoint opens the folder picker and then reports progress, and completion, as a toast that survives navigation. A finished export offers Show in Finder (macOS) or Show in folder (Linux) to locate the `.silo-backup` file; Silo only reveals exports it created and still tracks. Importing opens a dialog to validate the export, pick a sandbox when several are present, and name the new one, after which progress continues in a toast with Open to jump to the imported sandbox. Failures stay until dismissed and offer Retry; cancelling an import confirms first, since it removes the incomplete sandbox.
- 94d2c61: Refresh Silo with a frosted sidebar and title bar over a subtle teal background, clearer content surfaces, and coordinated light and dark navigation states. Tighten the logo spacing and keep submenu guides clear of highlighted items. Respect system preferences for reduced transparency and increased contrast.
- dd5cb50: Sandbox actions now wait their turn instead of failing when another operation is running. Silo shows what each pending action is waiting for: a per-sandbox "Waiting for…" status and a compact indicator listing running and waiting operations with elapsed time, flagging any operation that has been running unusually long.
- ae452d9: Confirmations and small forms (restore, fork, delete, new checkpoint, import) now open next to the button you used instead of blocking dialogs. Long actions show a progress notification with the current step, elapsed time and Cancel where possible, and end in a notification that stays until you close it.
- bd52631: After an upgrade converts your sandboxes to the new storage, Silo keeps the previous storage as a pre-upgrade backup, tells you its size and the date it will delete it (14 days later), and deletes it automatically. On Linux the backup can be as large as all your sandboxes, so you can free that space sooner: Settings, General, Storage has Show and Delete now. Silo only deletes it after every sandbox was converted, never after you continued past a failed migration.
- 1ddd345: System notifications now arrive as soon as something happens, and only while Silo is in the background: while you are using Silo, results appear in the app instead. Each notification names the sandbox, opens it when clicked, and replaces the older one for the same event. Silo no longer notifies you about a start, stop, or restart you asked for. Notifications are now grouped as Failures, Unexpected sandbox changes, and Long tasks finished, so work that took more than a few seconds tells you when it is done even if you have switched to another app. Failed pushes, GitHub changes, port changes, checkpoints, and log exports now reach you too.
- 14eed4e: Quitting from the menu bar now asks "Quit Silo?" and names the running sandboxes it will stop, with Quit and stop or Cancel. Silo quits without asking when nothing is running.
- fb493ca: Changes sent to another computer no longer wait behind every other remote change: changes to different sandboxes run at the same time, and changes to the same sandbox wait their turn in that computer's queue like local work. A queued change that has not started before its request expires, or whose computer disconnected, is dropped instead of running later, and Silo reports that nothing changed. After a brief connection loss Silo asks again for the same change and receives its result instead of running it twice. Both computers need this version of Silo.
- d654e4b: Upgrade the bundled VM runtime and guide existing sandboxes through a one-time data migration. Add manual checkpoints, stopped forks that require an explicit Start, and restore with a recovery checkpoint. A recovery checkpoint can be forked while its restored source awaits explicit Start. Brief background activity no longer makes a sandbox Start or Stop fail immediately. Preserve new backup history on later launches after migration completes. Restored forks receive their current host-side GitHub assignment before guest execution; checkpoint history never restores old grants. Imported VMs remain deny-all until current host policy authorizes network access, and can be re-exported without changing that isolation. Portable exports now use the new v3 snapshot format and include the owned workspace disk, including migrated VMs with implicit managed-disk defaults and runtime-assigned network details; exports retain their source ancestry and reuse the workspace's saved snapshot group across checkpoints, forks, restores, and backups, while imports retain the archive's group; older v2 archives are incompatible and are rejected. Restored managed root disks preserve the capacity recorded by the snapshot. Imported images rebuild cached disk descriptors against the destination cache so archive restore no longer depends on the exporting machine's cache paths. Keep opaque secret placeholders usable in unrelated agent requests while substituting real credentials only at their approved destinations. The supported existing-VM account migration uses MicroSandbox 0.7.2 snapshot commands and resumes from the recorded snapshot directory. Linux app packages load the verified bundled tools from a stable package path so runtime integrity checks remain valid.
- 5212281: Give each sandbox its own detail page. Click a sandbox in the overview to open a page with Overview, Checkpoints, Storage, and Access tabs, reachable through back/forward navigation and deep links. The list rows are now simpler: checkpoints, storage, export, and SSH controls moved onto the detail page, and the inline SSH button is gone while the SSH badges stay.
  
  Redesign the Checkpoints tab with relative timestamps, memory/disk/recovery tags, a New checkpoint form, a one-click Restore with confirmation, and per-checkpoint Fork and Export actions.
- d909819: The Backup tab is replaced by **Export…** on each sandbox and **Import…** in the sandbox list. Exporting saves a sandbox's disks to a `.silo-backup` file and no longer claims to stop running sandboxes, which it never did. You can also **Export** an individual checkpoint from a local sandbox's checkpoint list. Export and import results now stay until you dismiss them. Use checkpoints from a sandbox's menu for local rollback.
- 60b23ca: The sandbox page's Overview tab now lets you add, edit, and remove secrets and ports for that sandbox, and open a port, without leaving the page. New secrets are preselected for the sandbox, and the ports list matches the Network page.
- c84d354: Websites published from a sandbox now open at an address of their own, such as `http://dev-1a2b3c4d.localhost:43000`, so cookies they set stay separate from other local services and sandboxes. Safari and unrecognized browsers keep using `127.0.0.1`, and each port has a **Copy 127.0.0.1 address** action for development servers that reject other host names.
- 046fac8: SSH access to a sandbox on another computer now uses a key that belongs to the connecting computer; the owning computer's own key is never sent. Turning SSH access off revokes the keys other computers registered and replaces Silo's generated key, and removing a remote computer deletes this computer's SSH keys for it. Both computers need this version of Silo to connect over SSH.
- 22e0f36: Visual Studio Code now opens sandbox folders in a separate "Silo" profile, leaving your normal VS Code profile untouched. Those windows stop Git in the sandbox from using VS Code's GitHub sign-in and stop automatic port forwarding, so sandbox ports reach this computer only through Silo's published ports. The first time, VS Code asks to install Remote - SSH into the Silo profile.

### Patch Changes

- 1f384f9: An export or import that could not be resumed after relaunch no longer blocks sandbox startup recovery on every launch. Dismissing its failure stops retrying and keeps any files it left, which also unblocks app updates. Cancelling an export or import now works even when Silo cannot save its progress.
- 851a0b3: A failed or cancelled export or import now says only what is true: an export reports that no export file was saved instead of claiming incomplete files were removed, and an import reports that no sandbox was added, noting when data it unpacked may still use disk space. Cancelling from the operation queue is always reported as a cancellation.
- d1b1296: Failures to open a VM desktop, reopen GitHub authorization, open GitHub repository access, or run a terminal, editor, or other action on a VM on another computer now show an error notification instead of failing silently.
- 7fc3d20: Activity keeps a deleted sandbox's events, such as its failures and removal, unless a sandbox filter is selected.
- 9cd7cbd: Silo now shows a one-time notice that it is in alpha and can lose sandbox data, with a reminder to export sandboxes regularly and push work to GitHub. Dismissing it is remembered.
- 56d0788: When Silo runs as an AppImage, Visual Studio Code and Zed can reconnect to a sandbox after Silo restarts: their SSH settings now point at the AppImage file instead of its temporary mount, and settings written by earlier versions are updated at startup.
- 28d0b29: Initial setup and retries no longer overwrite sandbox changes made meanwhile. Setup applies its sandboxes as one atomic batch of targeted changes, and retrying a failed setup resumes the recorded attempt against the current settings instead of resending a whole list, so concurrent edits are preserved.
- 95dff1b: Automatic retry after a temporary failure now covers more safe actions: starting, stopping, or restarting a VM from a remote computer, applying a sandbox's GitHub Git identity, and saving a sandbox's secrets. Each retry re-runs the same intent against fresh state and only retries genuinely transient failures, such as a timed-out or momentarily unavailable runtime; validation, rejection, and verification failures still stop immediately. Port forwarding is unchanged: its failures are deterministic, so it does not retry.
- 9844a68: Push now uses a GitHub token that can only write commits to the one repository being pushed, and Silo revokes it as soon as the push ends. The token reaches Git through a private pipe instead of process settings that other programs could read.
- 3eede2b: Sandbox GitHub access now requests only the permissions it needs. Read-only access covers contents, issues, pull requests, statuses and checks. "Allow GitHub changes" adds write access to contents, issues, pull requests and statuses, plus workflow changes and read access to Actions runs. Sandboxes no longer receive administration, secrets, webhooks, environments or security-alert access, even when the GitHub App was granted them.
- d34c56e: Pushing one repository no longer makes pushes of other repositories fail with "Another host push is using the publishing cache". Only a second push of the same repository waits for the first to finish, and a cache still in use is never evicted to make room.
- f6aa4bb: A running push can now be cancelled from its notification, and quitting Silo can cancel it instead of waiting. Silo stops the Git transfer, cleans up the sandbox export, and reports "Push cancelled"; if GitHub was already receiving the push, the result asks you to check the branch on GitHub. Pushes also renew Silo's GitHub sign-in when it would expire soon and stop cleanly, rather than failing midway, if they outlast their GitHub credential.
- e1f34e4: Removing a secret no longer gets stuck when an assigned sandbox was deleted, is missing from the runtime, or is stopped but could not confirm the change: such a sandbox cannot use the secret, so the removal finishes. A running sandbox still has to confirm before the secret is deleted.
- 02e3831: Finding repositories in a sandbox no longer slows down or stalls Silo's status updates. Silo reads each sandbox's repositories in the background, one read at a time, shows the last known list meanwhile, and skips dependency folders such as `node_modules` and `.venv`. Refresh repositories still waits for an up-to-date list.
- a2bb52f: Pushing a new branch now counts only the commits GitHub does not already have, instead of the branch's entire history (for example "Push 2 commits" rather than "Push 3,412 commits"). Pushing a new branch also no longer resends history that is already on the repository's default branch.
- 930b1e9: When a push fails while reading from the sandbox, the error now says so in Silo's own words, and any text the sandbox's Git printed appears only in the details, labelled as output from the sandbox rather than from Silo or GitHub.
- 6924bf7: Silo no longer keeps checking other computers over SSH while its windows are closed or hidden; it refreshes as soon as a window becomes visible again. Visible windows check each remote computer once per interval instead of twice, and bursts of VM state changes trigger one refresh instead of one per change.
- d654e4b: Allow backups of migrated VMs when MicroSandbox assigns an IPv6 address to their network interface.
- 55a7f81: Cancelling a running operation now stops it cleanly. Cancelling a secret save stops the in-flight runtime command instead of letting it run to completion and then retry, and cancelling a VM start no longer reports the boot-time secret step as an unavailable runtime. Automatic retries of a start, stop, restart, or secret save now honour a cancel across the whole retry sequence, including during the wait before a retry. Cancelling a sandbox action no longer raises a failure notification, since a cancellation is an expected outcome. Start, stop, and restart also re-check the sandbox by its stable identity when their turn arrives, so a rename or removal while waiting is handled correctly.
- 730d8a2: Cancelling a guest command on a stopped sandbox now stops the sandbox again instead of leaving it running, and repository discovery no longer waits behind GitHub access changes.
- a874d39: Cancel now stops an export or import that is waiting for a runtime command left over from an earlier Silo session, and that wait gives up after two minutes instead of an hour.
- 6fa333f: Show progress immediately while creating, restoring, or forking checkpoints. Keep restore confirmation inline and enter fork names in a compact popover. Wait through brief background VM inspections instead of immediately reporting that another sandbox operation is running.
- e9987d7: Cancelling Create checkpoint no longer leaves a running sandbox paused, and a cancel that arrives after the checkpoint was already saved no longer reports it as failed.
- d719352: Exporting a checkpoint now records the workspace disk size, CPUs and memory the checkpoint was captured with, instead of the sandbox's current settings, so the imported sandbox starts and exports again after the original was changed.
- 0504ce2: Remove the redundant Fork current state button from Checkpoints. Fork saved checkpoints from their rows, or fork the current state from the sandbox menu.
  
  Remove the confusing Start restores captured session or disks note from stopped sandbox rows.
- ae20b1c: Forking a sandbox and restoring a checkpoint now confirm with a toast, since both leave the result stopped and otherwise easy to miss. A completed fork shows "Fork created" with an Open action that jumps to the new stopped sandbox. A completed restore shows which checkpoint was restored, notes that a recovery checkpoint was saved first, and offers Start, which runs the same guarded start as the sandbox page (respecting capacity and unavailable-operation notices). Checkpoint creation stays silent, since the new row is feedback enough, and errors continue to appear inline.
- 0f56d40: The Checkpoints section now shows what Restore and Fork do as visible text instead of only in a hover tooltip.
- d90bd02: Remove unused checkpoint data when a sandbox or its last fork is deleted. Recover interrupted checkpoint cleanup from operation journals at launch instead of deleting old snapshots by age.
- 0504ce2: Keep checkpoint actions available after Restore while the sandbox stays stopped. Create another checkpoint, restore a different point with a recovery checkpoint, or fork the selected state without starting the sandbox. Local and remote Start run the selected state consistently.
- 00aded3: Refresh sandbox state after checkpoint capture releases its operation lock.
- 3a40190: Checkpoint create and restore now wait only for other work on the same VM instead of blocking every VM, so a checkpoint on one sandbox no longer holds up actions on others. Per-VM ordering also keys on a sandbox's stable identity, so renaming a sandbox can no longer let two operations on it run at once.
- 2c549ba: Preserve existing credential proxy settings during workspace migration, and show the affected sandbox and safe cause if conversion stops.
- 9f62f58: Escape now closes confirmation popovers opened from icon buttons that also show a tooltip, and the popover stays inside the window.
- b6f55d4: Removing a secret, clearing a sandbox's GitHub repositories, and deleting a sandbox from the list now ask in a popover next to the button, with a clear question and Cancel, instead of tiny check and cross icons.
- fc08a97: Sandboxes converted to the checkpoint runtime could no longer start once the previous runtime folder was deleted, failing with a missing image file. Silo now points their image files at the converted runtime when it starts, and new conversions no longer depend on the previous folder.
- cb46bb7: After a sandbox change fails, the sandbox list shows the current state again instead of staying stuck on "updating" until Silo is relaunched. The interrupted change remains available to retry.
- 6216a29: While Silo works on one sandbox (starting it, creating or restoring a checkpoint), that sandbox keeps showing its last known state next to its progress label, and every other sandbox keeps refreshing; sandboxes starting at launch no longer delay the first list. A sandbox whose state cannot be read shows its last known status with a warning instead of the whole list failing to load.
- abed177: When a sandbox boots but Silo cannot confirm its state or record which secrets it started with, Silo now stops it again and reports the start as failed, instead of showing a successful start with outdated secret status.
- 4d6d25f: Refreshing sandbox state does less work: expired logs of stopped sandboxes are cleaned in the background at most once an hour instead of on every refresh, and two windows refreshing at once no longer scan the same sandbox for repositories twice.
- 8275adf: After starting, stopping, changing or checkpointing a sandbox, the GitHub panel, repository lists and push results no longer briefly disappear. A sandbox action or setup that succeeded is no longer reported as failed when Silo cannot refresh sandbox states right afterwards; affected sandboxes show their last known status with a warning instead.
- aa164f4: GitHub access updates now wait their turn behind other work on the same sandbox and show as "Applying GitHub access" in the operation queue, so a token refresh can no longer collide with an edit, checkpoint restore or removal. Starting a sandbox no longer waits indefinitely behind an access update, and a newer access choice replaces an older one that is still waiting.
- 536fdab: Checking whether Git identities are already set up no longer boots stopped sandboxes, and saving or checking Git identities now works on one sandbox at a time with a named, cancellable entry in the queue instead of holding up every other sandbox.
- d972eaa: A sandbox setup that was interrupted when Silo closed is now shown as interrupted even while sandboxes start at launch or Silo does background work.
- 07b8697: Sandboxes started when Silo opens now each show "Starting <name>" and can be cancelled like a normal Start, and other sandboxes' actions no longer wait behind each boot. A selected fork or restored sandbox that needs its first Start is now reported instead of being skipped silently.
- ce522af: Background sandbox health checks no longer time out and report "Health checks unavailable" when many sandboxes are configured; a sandbox that is slow to answer is skipped on its own.
- 5d4d19b: A cancelled start or restart is no longer resumed automatically the next time Silo launches, and one unreadable saved sandbox action no longer prevents the others from being resumed.
- 9e3898f: After an app update, Silo resumes exactly the sandboxes that were running before it; an earlier failed start waiting for Retry no longer starts its sandbox at the next launch.
- 835325a: A saved sandbox action that can't resume at launch no longer stops Silo from resuming sandboxes after an update or starting the sandboxes selected for launch; it is reported, and starting or stopping that sandbox again replaces it.
- 364c348: Cancelling a restart while the sandbox is stopping no longer interrupts the stop; the cancel takes effect before the sandbox starts again.
- 255709e: Cancelled start, stop and restart actions are saved as cancelled, so they no longer reappear as red failures after a reload, a relaunch or on another computer. Sandbox activity written by a newer Silo version no longer stops sandboxes from starting or stopping.
- 1756566: A sandbox action that retries automatically keeps its original start time in the queue, so it is flagged as taking longer than expected instead of appearing to restart with every attempt.
- 4137c73: A sandbox stop that times out and never settles now fails once with a clear message instead of being retried for several more minutes.
- 9f52720: While Quit stops local sandboxes, its overlay names the sandbox being stopped and how many remain, for example "Stopping dev (1 of 2)…".
- 8d85b28: Quitting Silo no longer runs a stop, or records a "Sandbox stopped" activity entry, for sandboxes that are already stopped.
- 93d36c2: When Quit stops local sandboxes but then fails, an automatic retry or a queued start from before Quit no longer starts a sandbox that Quit had just stopped.
- f539dec: A change sent from another computer no longer silently discards a local sandbox change that is waiting to be retried; it is refused until the local change is retried or corrected.
- 925a5ce: Cancelling a start just as it leaves the queue now cancels it instead of reporting that the operation can't be cancelled.
- 7ca9b3d: A runtime migration that fails, including when its failure cannot be saved to a full disk, now shows as failed with Retry and Continue available instead of staying "running" until relaunch.
- 1c3fb5f: Logs and sandbox activity now hide more kinds of secrets, such as `AWS_SECRET_ACCESS_KEY=`, `api_key =`, `Password:` lines and whole private key blocks, and strip more terminal control sequences from runtime output.
- 883bd33: Sandbox start, stop, restart and setup failures now show one line that says what happened and what to do, without exit codes or raw runtime output; the runtime's own explanation is kept separately for a details view. A configuration change that fails after some sandboxes were already changed now keeps its precise reason and says that the completed changes were kept.
  
  Older activity failures also keep process details behind the diagnostic field, and runtime launch failures explain how to retry.
- 8fec86b: Starting, stopping or restarting a sandbox on another computer twice in a row no longer reports the second request as a failure; it joins the one already waiting.
- 9937208: Sandbox secrets and GitHub access credentials no longer pass through the bundled runtime's environment, where other programs running as you could read them and where a secret named like a system setting could change the runtime's behaviour. Silo now hands them to the runtime privately when it starts, updates or restores a sandbox. Secret-name rules are now defined once and shared by the secret form and the app.
  
  General secrets use generated runtime source names while preserving their names inside the sandbox.
- df8cfe7: Debian packages build again with the bundled tools in `/usr/libexec/silo/tools`, and a package upgrade now waits until Silo's local VMs have stopped instead of replacing the running runtime.
- f197f86: Sandboxes that were never started can be deleted again. Deleting one used to fail, so a setup that stopped before the sandbox's first start, or an import that was interrupted, could leave a sandbox behind that kept its name. The bundled runtime now removes such a sandbox together with its disk, and an interrupted import no longer asks you to pick another name.
- dc3cf26: Deleting a sandbox now says what is lost: its files and checkpoints are deleted and this can't be undone. The confirmation reads the same from the sandbox list, its menu and the sandbox page, and SSH sandboxes explain that nothing on the host is deleted.
- 41b2159: A sandbox desktop now appears inside an amber frame marked "Sandbox content", so anything drawn by the sandbox, including windows that look like Silo's, is clearly separate from Silo's own controls. Pages in the sandbox desktop can no longer read or write this computer's clipboard.
- d7c98aa: Long desktop setup and update actions are no longer reported as stuck while they are still within their normal duration.
- d654e4b: Connect local and remote VM desktops through the existing private SSH transport so restored desktops remain viewable without live port publishing.
- cd4b3b6: The desktop viewer's connection to a sandbox no longer opens a local network port: it now uses a private socket that other programs and accounts on this computer cannot reach or take over.
- ca21341: A sandbox desktop's display no longer stays disconnected after a few unrelated streamer crashes hours apart: a display that ran for a minute gets a fresh set of automatic retries, and retries now wait a few seconds between attempts. Newly installed or updated desktops get this behaviour.
- 44da567: If Silo crashes or is force-quit while a sandbox desktop is open, the desktop's background connection now ends too instead of running on and holding the sandbox's connection. A connection whose sandbox stops answering also closes after about 45 seconds so the viewer can reconnect.
- 6872e33: Updating a stopped desktop now refreshes its lifecycle helper together with the streamer, so existing sandboxes receive current desktop controls without restarting the session.
- b5c4627: Closing a desktop window while it is still connecting no longer freezes Silo.
- 0b42e39: Opening a desktop twice in quick succession no longer creates a duplicate window, and a missing OpenSSH client is now explained when opening a desktop.
- a59c27c: The desktop viewer now connects once the display stream is ready instead of failing with "The desktop is not running.", and a failed agent-tool setup no longer makes the desktop unavailable.
- b8c17a1: Deleting a fork or restored sandbox whose checkpoint start could not be verified now also removes the VM that start created (stopping it first if needed), instead of leaving it behind in the runtime. A damaged checkpoint history no longer blocks deleting its sandbox.
- 9fb6bc8: A damaged checkpoint history file (or one saved by a newer Silo) no longer stops Silo from loading every sandbox: only that sandbox is flagged, and the others keep working.
- 020dff4: Polish found in end-to-end use: identifier fields no longer auto-capitalize or autocorrect names, checkpoint names use English dates, operations with their own notification no longer show a duplicate queue toast, progress notifications are aligned and clearly indeterminate, notification actions share one button style, sandbox menus (Fork, Delete) open reliably and close on Escape or navigation, list rows confirm deletion in the same popover as the detail page, and a deleted sandbox's notifications are dismissed.
- 800349b: Opening a desktop or authorizing a remote editor no longer waits for another editor connection to finish, and one failed editor preparation no longer blocks editor connections until Silo restarts.
- eb6739a: Opening a sandbox in your code editor now works when `~/.ssh` or `~/.ssh/config` is a link managed by a dotfiles tool such as stow or chezmoi: Silo updates the linked file and keeps the link. When the linked file can't be changed, such as a home-manager file, the message shows the exact line to add yourself.
- a98d9c7: The sandbox editor now locks its fields while a save is in progress, so changes can no longer be typed and then silently lost.
- 9776bad: Editors that reconnect on their own after an upgrade now open your current sandbox instead of the copy kept from before the upgrade. Before, an editor window restored by VS Code or Zed kept using the old storage until you opened the sandbox from Silo again: it could start the stale copy, and it stopped connecting once that backup was deleted. Silo now points those editor connections at the upgraded storage at launch, adding one line to your SSH configuration when needed. Your existing lines stay, and the pre-upgrade backup is not touched. When your SSH configuration links to a file Silo can't change, a notice in Silo shows the line to add, with a Copy button, until you add it or dismiss it.
- 1587520: State exports stop creating hidden snapshots after 128 captures per sandbox. Exporting an existing checkpoint remains available; existing captures are preserved because later checkpoints depend on them.
- 02ffd18: Sandboxes with assigned secrets, a custom network policy or published ports can now be exported; the export leaves those settings out, as an import never used them. When a sandbox still cannot be exported, the message names the setting that blocks it.
- 5b3ca38: A verified export is no longer reported as failed ("history update failed") when Silo cannot update its old list of past exports, which no screen showed; Silo no longer keeps that list. A damaged or newer export settings file no longer blocks every export and import; Silo only forgets the last export folder.
- b899f7c: Exports to network shares and exFAT drives no longer fail at the last step with "Invalid argument"; Silo still never replaces an existing file there.
- 2b15ffa: Sandboxes created with the current runtime can be exported again. Exports made earlier still import.
- acdb521: Export file sizes are now labelled GiB and MiB, matching how they are measured and the Storage panel.
- c7cc753: Exports check estimated free space for the working copy, native capture, and destination before capturing or saving data. Copies sharing a volume are counted together, and the destination is checked again using the final archive size.
- dcf2ce3: Silo no longer freezes when the last export folder is on a stalled network share or a sleeping disk: refreshing export and import status no longer measures that folder, and cancelling or dismissing an export or import no longer writes to disk on the window's main thread.
- 285b7e1: When an export file comes from an older or newer Silo or runtime version, the import now says which version can open it (update Silo, or import it with the version that created it and export it again) instead of a generic "not supported" message.
- 9e8812c: Quitting Silo from the Dock or with an AppleScript `quit` now goes through the same Quit as the Silo menu, so settings are saved and local sandboxes stop. Logging out, restarting or shutting down (macOS and Linux) and a SIGTERM now stop local sandboxes gracefully without asking, within the time the system allows.
- 5e5b266: On Debian and Ubuntu, updating Silo from the app now authenticates, checks Silo's software source, refreshes the package list and downloads the update before stopping any sandbox. Cancelling authentication, a disabled source, a busy package manager or a release that is not yet available no longer stops and restarts your sandboxes, and an update that waits too long gives up after 30 minutes without stopping them.
- ac99855: After an in-app update on Debian and Ubuntu, Silo now closes its SSH tunnels and listeners before restarting so the new version can reuse their ports. If Silo is updated but cannot restart itself, it resumes the sandboxes it stopped and asks you to quit and reopen Silo instead of offering to install the update again.
- a9a4b30: Following logs now reads only the records written since the last refresh instead of rescanning every retained log file every three seconds, and no longer inspects the sandbox or cleans up expired logs on each refresh. Silo also keeps fewer log search snapshots in memory.
- 76c8f91: One malformed or oversized log record no longer makes a sandbox's logs impossible to search or export. Such records are shown as placeholders or truncated, Logs says when some records could not be read, and times written by a sandbox to its console are labelled as reported by the sandbox.
- 2448ddb: Only one Silo runs at a time. Opening Silo again brings the running window forward, including when it is hidden in the menu bar or tray, instead of failing to start. Opening a different Silo build while one is running shows "Silo is already running. Quit it first." A startup failure now explains itself in a dialog instead of closing without a message.
- a2c3dc8: Quitting while onboarding is still creating a sandbox or verifying GitHub access no longer leaves Silo stuck on "Stopping local sandboxes…". After a few seconds Quit saves settings and continues; the Quit screen names any sandbox work it is still waiting for and offers to cancel it.
- 40d299f: On Linux, opening Silo while its package update is still running, or after an update was interrupted, now shows a dialog instead of doing nothing. An interrupted update names the command that finishes it (`sudo dpkg --configure -a`).
- e272369: While an update is ready to install, Silo no longer makes unrelated actions (such as forking a sandbox or changing GitHub or secret settings) fail intermittently with "Silo is installing an update" or "Secret settings are busy" while it checks whether the update can be installed.
- 3954484: Checking for updates again no longer discards an update that has already been downloaded and verified. Automatic checks now also run while an update is available but not yet downloaded, so a newer release replaces it.
- d4accba: Restarting to finish an app update no longer shows the "Stopping local sandboxes…" Quit screen or starts a separate sandbox shutdown while the new version launches.
- 31a0cf2: On Linux, Help → Documentation now opens Silo Help in its own window instead of the default browser, which fixes "File not found" with Ubuntu's snap Firefox. Links to the web still open in your browser.
- 2083a79: On Linux, the Virtualization check now explains when hardware virtualization is turned off in firmware instead of suggesting a retry, and it now detects when another hypervisor such as VirtualBox or VMware prevents KVM from creating VMs, before the first sandbox fails to start.
- eaad1b3: On Linux Wayland desktops, clicking the Silo tray icon now opens the Silo window instead of a status panel that Wayland could place anywhere and that might not close.
- 6c40073: On Linux, Silo notifications now name the installed Silo desktop entry and icon, so desktops such as GNOME show them as coming from Silo and list Silo in per-app notification settings.
- 3ca3563: The update's release notes link now opens in the browser chosen in Settings, and on Linux it no longer leaves a defunct helper process behind.
- e36f766: On Linux, moving the Silo window with Alt+drag no longer opens the hidden menu bar when Alt is released. Only a quick tap of Left Alt toggles it.
- 3fc8021: On Linux, the Silo window no longer pops up whenever the desktop's tray briefly restarts (for example a Plasma or GNOME Shell reload). Silo shows its window only if the tray stays unavailable for a few seconds while the window is hidden.
- 5b66da1: If Silo refuses a settings change as invalid, the change is now undone and reported instead of being retried forever. Later settings changes save normally, and Quit is no longer blocked by the refused change.
- 8808dc8: A failed or cancelled import now removes the snapshot data it had already loaded, instead of leaving several gigabytes in Silo's runtime storage.
  
  Cleanup also removes multi-snapshot import chains in their dependency order.
- 2c48ebf: Exports and imports spend less time reading disks: an export reads each captured snapshot once less, and importing one sandbox from a multi-sandbox export unpacks only that sandbox.
- 50bfc1b: Browsing the same large folder in two Silo windows no longer makes either listing expire.
- e5b23dd: A sandbox folder containing a file name that is not valid UTF-8 now lists normally, with that name shown escaped, instead of failing with "Could not load this folder." Very large folders now report "This folder is too large to list." instead of a generic failure.
- 6872e33: Fix desktop viewer proxy connections that could close before receiving request headers on macOS.
- 9a24d1c: Fix repositories disappearing with a repository-read warning when a sandbox contains more than 200 Git repositories or worktrees. Repository reads no longer start a sandbox that has stopped.
- 5cc1ee5: Fix sandbox startup in optimized local macOS builds by applying and verifying the VM helper's existing engine-specific signature during packaging.
- debad79: Show log skeletons in place, keep loaded history and scroll position when returning to Logs, and load older records automatically while scrolling. Identify remote sandboxes with a server icon in their badge and reveal the computer name on hover or keyboard focus. Expand or collapse each complete log inline with the disclosure control at the end of its row.
- e166b5b: Finish interrupted exports and imports before starting runtime migration, so a saved operation no longer makes the first migration attempt fail.
- d9a18e5: Remove secrets immediately from the list and credential store even when a sandbox is unreachable. Retry revocation in the background and warn on affected sandboxes with a Restart action, including remote sandboxes. Keep replacement secrets safe when a removed name is added again.
- 24f34a5: Disable deletion of running VMs with a stop-first explanation. Show failed sandbox changes and let users dismiss the error to recover sandbox controls.
- 52c599b: Fix Start sandboxes at launch saving only the switch without its default sandbox selection. Existing enabled settings with no saved selection now start the default local sandbox.
- 135cd52: Hide the sidebar submenu guide behind selected and hovered navigation items.
- 43c2305: Forking a running sandbox's current state no longer makes every other sandbox wait while its memory is saved; only the source sandbox is busy until the fork is added. A fork that fails partway now removes everything it added and reports the original error.
- 516b86b: The Fork form now checks the new sandbox name as you type, using the same rules as creating or importing a sandbox, and explains an invalid or already used name inline instead of failing after submit.
- 8c8d5a5: The Fork form for a sandbox's current state now says that Silo adds a "Fork point" checkpoint to the source sandbox's history and briefly pauses a running source while saving it.
- d2d2fc8: Opening a sandbox terminal in Ghostty no longer freezes Silo while macOS asks for Automation permission or Ghostty is slow to respond.
- ff7a6ec: Align the all-repositories option with the GitHub authentication choices for a more compact access editor.
- f97cc4e: Connecting GitHub on macOS no longer fails with "Invalid GitHub callback" when the browser opens the connection back to Silo slightly before sending the authorization.
- 598cdfc: While Silo waits for you to authorize GitHub or install the GitHub App in the browser, GitHub access renewal, repository refresh, host push and app updates keep working instead of waiting up to ten minutes.
- 0e12642: Disable access on the GitHub page now also detaches VMs that use a personal token, so no VM keeps GitHub access while it is off.
- 375cec6: Disconnecting GitHub after the sign-in token expired now renews it first and then revokes Silo's authorization on GitHub, instead of treating GitHub's answer for the expired token as success and leaving the authorization active.
- 2e21b48: Deleting a sandbox now removes its GitHub repository access and revokes the GitHub tokens issued to it. A new sandbox created with the same name starts without any GitHub access instead of inheriting the deleted sandbox's repositories and write permission.
- a71d1b5: Silo now reads your host Git author at most once a minute in the background instead of running Git on every refresh, so GitHub actions no longer wait for it. On a Mac without the developer tools, Silo no longer asks to install the Command Line Tools just to look up your Git author.
- 030335a: An unanswered Keychain permission prompt for the GitHub personal token or sign-in no longer blocks saving GitHub settings, Disable access, or Disconnect while it waits.
- b4b33b0: Removing GitHub access now detaches every VM even when one VM fails, and a failure on one VM (including a deleted one) no longer blocks GitHub access for the others.
- 9236665: GitHub authorization and repository access pages now open in the browser chosen in Settings. On Linux, opening them no longer waits for the browser, which could stall connecting GitHub.
- 793d21d: A GitHub rate limit now only delays requests made with the credential that reached it. A limit on the GitHub OAuth connection no longer makes the personal-token check fail and detach sandboxes that use a personal token, and a personal-token limit no longer delays OAuth access.
- 2274ff9: Reconnecting GitHub no longer fails when one VM cannot be updated or was deleted; the failing VM shows its own error and the others receive access.
- 05e1f09: When the system credential store cannot save a renewed GitHub credential, Silo keeps using the renewed credential and retries saving it instead of failing every GitHub request.
- 6696cc4: GitHub requests that could not connect are retried automatically, and an expired GitHub sign-in that can be renewed no longer shows as disconnected.
- 27b609f: When connecting GitHub fails or is cancelled after you authorized Silo, Silo now revokes the new authorization instead of leaving it active on GitHub. Reconnecting also revokes the authorization it replaces: only the old token when you reconnect the same account, or the whole previous authorization when you switch to another account.
- 92c80c7: The GitHub page and personal-token settings now say "sandbox" consistently instead of mixing in "workspace" and "VM".
- 7d0a9cb: Changing a sandbox's GitHub settings now saves only that sandbox. A fork's copied GitHub access is no longer dropped by an edit made on an older view of the GitHub page (the edit is refused with a message to review and retry), and a settings change or setup made right after Disable access no longer turns GitHub access back on.
- e2924f9: Saving GitHub settings, Disable access, Disconnect and cancelling a connection no longer wait while Silo applies a Git identity or GitHub access inside a sandbox (which can take minutes when the sandbox has to start). Forking a checkpoint no longer fails with "GitHub settings are busy".
- c3fac38: Simplify the GitHub token option by showing its access explanation when hovering the label, without a separate info icon.
- 1b6a2fd: Personal GitHub token checks no longer stop retrying after a network outage, and saving the token again always re-checks it.
- f06d016: Silo's GitHub background work now sleeps until it has something to do instead of re-reading the GitHub settings ten times a second, reducing idle CPU and disk use.
- 464192e: Silo no longer sends an existing user back through setup when its settings file cannot be read or is damaged; it opens the main window instead.
- d57b0fa: Setup now starts from the sandboxes that already exist on this computer once they load, instead of the default sandbox, and it never deletes an existing sandbox you did not delete yourself: if setup no longer lists one, it asks whether to keep or delete it first. Retrying a failed setup step now uses your current choices (for example edited Git identities or repositories) instead of repeating the failed request.
- a3ec9ab: When Silo cannot read its sandbox migration status at launch, the "Migration status is unavailable" screen now offers Retry instead of leaving the app blocked. Normal launches no longer flash "Updating your sandboxes" before Silo confirms no migration is needed.
- d18d1a4: Connected computers now refresh independently: one slow or offline computer no longer freezes the status of the others or delays network checks, and a computer that stops answering shows its last known state as refreshing. Removed computers no longer reappear, saved remote edits no longer briefly revert, a newly connected computer is listed as soon as connecting finishes, and repeated actions on a remote sandbox are no longer ignored while its status refreshes. A failure to read the list of computers is now reported as such instead of as a remote management error.
- c949ea0: A push whose status Silo cannot confirm no longer shows as pushing forever. Silo checks less often while the host does not answer, then shows the push as unknown with a reminder to check the branch on GitHub; acknowledging it re-enables Push. A push on a deleted sandbox or removed computer stops being checked.
- 82cfbb0: When Silo could not subscribe to live updates at launch, Retry now restores live updates, background refresh and refresh-on-focus instead of loading the data once and then going stale.
- 8f5994f: Silo paints its window as soon as it opens and runs its startup checks side by side instead of one after another. If startup fails, the error now offers Retry instead of requiring a quit from the menu bar, and an unreadable saved sandbox list no longer stops Silo from starting.
- b5452c6: Quitting during setup no longer waits up to five minutes for GitHub to confirm sandbox access; that check stops and can be repeated after reopening Silo. While Quit waits for setup work that is already running, the overlay names it (for example "Finishing setup (creating sandboxes)…") instead of "Stopping local sandboxes…".
- 01af00f: Retrying sandbox setup, or continuing setup while a sandbox is still starting, now runs the retry again instead of silently reusing an earlier successful attempt.
- ee4fc96: "Retry checks" runs the dependency checks again after an earlier check timed out without answering, instead of showing "Checking…" and timing out every time until relaunch.
- bfbdabe: When this computer's sandboxes are busy updating at launch (for example while resuming sandboxes), connected computers now appear right away with a note that local sandboxes will follow, instead of a loading skeleton for the whole update.
- 5479063: When a sandbox fails, is still starting, or cannot be confirmed after setup created it, the setup Review step now says which sandbox keeps Finish unavailable and offers to start it or check again, instead of "Not started · Continue to start this step".
- adfbaf4: A connected computer running a newer Silo no longer shows as unavailable when it reports a state or detail this version does not recognise; its sandboxes stay visible, with anything unrecognised marked as needing a Silo update. An unreadable activity or push entry is now skipped instead of making the whole state unreadable.
- a83b316: When the menu bar panel cannot load Silo's state, it now shows a panel-sized error with Retry, Open Silo and Quit instead of a clipped full-window error. While the panel is still loading, Open Silo and Quit stay available.
- 733c37b: When Silo cannot read its GitHub settings, the GitHub page now shows that problem instead of keeping the last connected state and repository list.
- 5d7bc31: While quitting, the overlay no longer shows an outdated "Waiting for…" label when queue updates answer out of order.
- 015e999: If Silo cannot start quitting from the main window's status menu, it now shows why instead of doing nothing.
- abd49d8: While a retried start, stop or restart waits its turn, the sandbox no longer flips back to showing the earlier failure when its status refreshes.
- f34b6e9: An action queued behind another waiting action now says what it is waiting for (for example "Waiting for Installing update…") instead of a bare "Waiting…".
- 4bc8246: Dismissed export and import results no longer come back after relaunch, including a previous result replaced by starting a new export or import. If Silo cannot dismiss a result, it stays visible and Silo says why.
- 7da6337: A cancelled start, stop or restart now still shows as cancelled (not failed) after reopening Silo, and a sandbox on a computer running an older Silo no longer shows a cancelled action as a red failure.
- 3c923ee: A momentary failure to read export or import progress no longer turns a running export or import into a fake "failed" result; Silo keeps showing its progress and reports the read problem if you try to start another one.
- 172f018: Starting, stopping, editing or checkpointing a sandbox no longer briefly shows GitHub as disconnected or hides its repositories and push results.
- 0ec6953: A finished sandbox action no longer stays hidden when a later status check arrives while another operation is running; the most recent real state is shown.
- 2cfa660: Continuing setup no longer turns GitHub access back on after you disabled it.
- 5360b64: Host push is more robust: stray files such as .DS_Store in the push cache no longer break every push, a damaged push history no longer hides the rest of the app state, a crashed push no longer stays "pushing" forever, retries after a remote rejection reuse the already-imported cache, and on Linux pushes trust the system certificate store when it is present.
- 2f61ce8: Host push now requires the sandbox to be allowed to push to the repository (read-only access is no longer enough), and matches repository names regardless of letter case.
- 34cb2bd: Saving settings that require stopping a running sandbox now asks first: **Stop and save…** shows an inline confirmation naming the sandbox before it is stopped.
- 259364a: Stopping or restarting a running sandbox now asks first everywhere in the main window, as the menu bar already did: the Stop button in the Sandboxes list and on the sandbox page, Restart… in the ⋯ menu, and Stop/Restart in the command palette show "Stop dev? Running processes will be interrupted." with a destructive confirm button. Starting and opening never ask, and menu labels end with "…" only when a question follows (for example Delete…).
- 390546a: Starting a sandbox from the command palette or a notification now gets the same checks as the Sandboxes page: Silo reports when VM operations are unavailable instead of trying anyway, and asks before starting under high memory pressure. That question now opens next to the Start button you clicked (or inside the palette) rather than at the bottom of the page.
- 6196930: The status panel now asks before starting a sandbox under memory pressure and reports unavailable VM operations beside its controls. Confirmed actions recheck the current sandbox status.
- 1ea3b51: Deleting a sandbox now asks "Delete dev permanently?" with a **Delete permanently** button, the same from the Sandboxes list and the sandbox page, and states how much disk space its files use and how many checkpoints go with it when Silo can read them.
- b6c2dd5: Offer Export, then delete from the sandbox deletion confirmation. Delete only after that export completes and is verified; cancellation, failure, or a sandbox becoming busy keeps the sandbox.
- 9a07b38: Quitting Silo with ⌘Q, the Silo or Dock menu, or a window close without a menu bar icon now asks "Quit Silo?" and names the running sandboxes that will stop, with **Quit and stop** and **Cancel**. Nothing is asked when no sandbox is running.
- 7d30ace: Deleting a sandbox from its own page no longer leaves a Back step that returns to the deleted sandbox and bounces forward again; its history entries become the Sandboxes list and Forward history is kept.
- 67ba843: The editor folder picker on the Sandboxes page now closes when its sandbox stops or when you navigate elsewhere, and it no longer reappears later over the page you moved to.
- 86dd020: You can now open a sandbox's page while it is starting, stopping, taking a checkpoint, or while its computer is refreshing or offline, to follow progress and errors; only the actions that change the sandbox stay disabled.
- 00ec1dc: Progress and Cancel for long operations now stay on screen while you move to Files, Logs, Network, Activity or settings. When several operations run, Cancel names the one it stops.
- e96da0d: The Files page's Push button is disabled while its sandbox is stopped, busy, out of date, or blocked by a system issue or sandbox change, matching the menu bar panel. Screen readers now hear which repository and sandbox each Push button belongs to.
- 80ac427: Start, Stop, Restart and Open now follow the same rule in the Sandboxes list, on the sandbox page, in the command palette and in the menu bar: a crashed sandbox can be started or restarted from its page, Stop waits until a start finishes, and an error notice no longer blocks stopping a running sandbox. Disabled Terminal, Editor, Start and Stop controls now say why when you hover or focus them.
- 285b935: Sandbox badges in Logs, Network, Files, Activity and Secrets now show the computer inline for sandboxes on another computer (for example "dev · Office Mac"), so same-named sandboxes are easy to tell apart. Each state also has its own shape (circle running, triangle starting, square stopped, cross failed), and the tooltip names the state.
- 8547448: With no sandboxes, Files, Logs and Network now say "No sandboxes yet" with a New sandbox button instead of asking you to select a sandbox. Empty lists across Files, Logs, Network, Activity and Secrets share one style, and an inverted log date range shows as a clear inline error.
- 73b8a74: When the Sandboxes or Settings menu is collapsed in the sidebar, its item now shows the sections' activity spinner, and Sandboxes shows how many sandboxes need attention, so a failure is visible without expanding the menu. Sidebar counts now read "1 sandbox has an error" or "3 sandboxes have warnings".
- 241e642: A sandbox's ⋯ menu is now the same in the Sandboxes list and on its page: the page gains Checkpoints, Storage and Add Linux desktop, and both menus disable the same actions while work runs or the computer is offline. The list's menu stays open for navigation during that work.
- 7f32f81: The command palette can now open a sandbox's page and its Checkpoints tab, fork, export or delete a sandbox, and create a new sandbox. "Open dev in {editor}…" asks for a folder first, like the editor buttons, and Fork and Delete open the same popovers as the sandbox page. Sandboxes with the same name on different computers no longer share one palette entry when choosing with the keyboard.
- 7c9a183: Saving a sandbox change that turns out to change nothing no longer leaves the list stuck on "Applying sandbox changes". A save rejected because the sandbox changed meanwhile now shows the latest reported state instead of an older one, and a push in progress stays visible while sandbox changes are reported.
- 300ec75: Silo does less background work: the Files tree refreshes the folders you can see with one timer instead of one per open folder, and a sandbox's page stops refreshing its ports while you are on another page.
- edac4c1: New sandboxes now start with CPU and memory settings this computer can run, the editor only offers presets up to its CPUs and memory, and a ceiling above them is explained before saving instead of failing.
- 411f6b1: When a sandbox changed elsewhere before your change was applied, Silo now always tells you the change was not saved, including **Add Linux desktop** from the sandbox menu and edits whose editor had already closed.
- 1c70d75: The status menu now shows a failed start, stop or restart as an error on the sandbox (and in the menu bar icon) instead of a neutral "Stopped", keeps cancelled actions neutral, and shows sandbox changes waiting for approval with a **Review** action.
- cad5d7e: Custom CPU, memory and storage values in the sandbox editor accept only whole numbers within what Silo supports (up to 255 CPUs), keep what you typed visible, and explain the allowed range in plain words instead of technical validation messages.
- ce84b5b: Reduce motion, in Silo's settings or the system, now also stops popovers, menus and dropdown lists from animating.
- 654d27a: The status menu's **Open site** submenu now copies each site's own address (for example `http://127.0.0.1:3000`) instead of a base URL without a port.
- 9e5b6eb: If another sandbox change starts while you are editing a sandbox, Save is now disabled with an explanation instead of silently doing nothing, and your edits are kept until you can save them.
- 186fc35: When a sandbox changed elsewhere while you edited it, **Review changes** now keeps your edits on top of the latest settings, lists fields changed on both sides with both values, and takes other changes along instead of discarding your draft.
- 5c21f20: Sandboxes on other computers no longer show a reorder handle, since their order belongs to that computer; reordering and its screen reader announcements count only this computer's sandboxes, and quick repeated arrow-key moves no longer repeat the first move.
- dee6b5a: When saving a sandbox's settings fails validation, focus moves to the first field that needs attention, and screen readers read each field's error with it.
- fb03ef0: Secondary text and small badges in the light theme are slightly darker, so they meet the WCAG AA contrast minimum on grey backgrounds.
- f65f651: An unsaved sandbox edit is no longer lost when you switch sections with the sidebar, shortcuts or the breadcrumb: returning to the sandbox list or page reopens the editor with your changes.
- b7b564d: When choosing a terminal, code editor or browser fails, Silo now explains why beside the setting instead of silently keeping the previous application.
- f082be1: The status menu's folder picker keeps **Open in {editor}** available when a background refresh fails and the previous folders are still shown.
- f346542: Edit, Add Linux desktop and Delete are no longer offered while a sandbox is starting, stopping or restarting (which Silo would then reject); they explain that the sandbox must finish first, and an open editor's Save waits until it does.
- 220e793: A failed push from a sandbox on another computer is now titled "Push failed · {sandbox} on {computer}" in the status menu instead of showing an internal identifier.
- bc7f023: File, folder and repository names from a sandbox now show hidden and text-direction characters as visible markers (such as `⟨U+202E⟩`), so one name cannot pass for another. The status menu names a repository you can push by its full path.
- b95164c: Long sandbox start, stop and setup errors now show a one-line summary with the full runtime output behind an expandable Details section you can copy, in notifications, the sandbox list and Activity. Exit codes and raw command output no longer fill the error itself.
- 6e19fdf: Cancel disappears from an import once it starts saving the new sandbox, instead of accepting a late Cancel that still ended with "Imported". The cancel confirmation now says what actually happens: "Stop importing? No sandbox is added." or "Stop exporting? No export file is saved."
- d8231b7: Closing "Checking export" while Silo checks a large export file now stops the check, and the import review no longer reopens minutes later when the check finishes.
- 2ef5475: Importing a sandbox no longer fails with "malformed VM data" on computers with many sandboxes. If the runtime's list is too large to check safely, the import stops with a clear size-limit message.
- f251df4: Imports reject snapshot mount policies that differ from the export manifest, including policies the manifest leaves at their runtime defaults.
- c7353bd: The Open action on an "Imported" notification now always appears and finds the new sandbox when clicked, even when the sandbox list updates a moment after the notification.
- feedd8a: Imports now check an export file's snapshot before handing it to the runtime. Files with links, special entries, unsafe paths, too many entries, or more unpacked data than the runtime storage can hold are refused before anything is written to the runtime.
- 8809fd1: Imports now confirm that the snapshot inside an export file matches the settings the file declares. A file whose snapshot adds mounted volumes, a different image, a default user or other undeclared settings is refused, and the data it loaded is removed.
- 9ffdec4: A checked export file is now labelled "Intact archive" rather than "Verified archive". The check confirms the file is complete, not that it came from a trusted source.
- 84b8700: An import interrupted by Silo closing now removes its partly loaded snapshot data on the next launch.
- 2cb2774: An export or import that was interrupted just before an upgrade no longer holds back the sandbox storage upgrade. Silo settles it without writing anything to your previous sandbox storage, which stays exactly as it was, keeps a finished export file, and tells you when you need to run it again.
- d2d0f2b: What an export or import that was interrupted just before an upgrade left behind is now removed from your upgraded sandbox storage once the upgrade is done, instead of staying there with everything else that was copied. The previous storage, kept as the pre-upgrade backup, is not changed. An export or import record that Silo can't read no longer holds back the upgrade: Silo sets the file aside in its data folder, never deletes it, and tells you to run the export or import again if one was running.
- 0429b23: When Silo closes during an export or import, the next launch no longer repeats the whole operation or starts sandboxes that were running when it began, and no longer holds back sandbox startup while an export is checked. An export whose file was already saved is verified and reported as complete; otherwise the incomplete file is removed and the export is reported as interrupted. An import whose sandbox was already saved is reported as imported instead of failing with "already exists"; otherwise its partial sandbox record is removed and the import is reported as interrupted.
- 0075239: Keep separate 125 MiB retention budgets for execution records and console output so a console flood cannot erase execution history.
- aa424e3: Use sandbox consistently for managed Linux environments.
- 0d68c01: Use Export, Import, and export file throughout sandbox transfers.
- 159e365: Name remote computers in connection and sandbox errors.
- 46ec20b: Explain frontend failures and give a reachable next step.
- 5b84607: Align CPUs, Memory, and Disk labels and binary units; explain resource limits.
- 67284bf: Explain Duplicate settings and Fork in menus and dialogs.
- 06b6b61: Use the same Linux desktop action labels across frontend surfaces.
- 18bdfc0: Use Updating and Offline consistently for remote computer states.
- c27a1f3: Align port addresses, states, and Open in browser labels.
- c2a22e7: Distinguish SSH hosts, SSH access, and Remote Login; split sandbox counts.
- 5722c24: Use Git identity consistently during setup.
- 2893e49: Rename the sidebar overview destination All sandboxes.
- 70ccdd7: Standardize failure wording and date validation; document the copy rules.
- 12dc7ee: Use Open Silo without an ellipsis for direct navigation.
- a4f2a62: Call log exports Save logs and distinguish sandbox transfer notifications.
- ae3c8d8: Show connection removal, duplication, and checkpoint deletion explanations inline.
- 3ca76f3: Remove a failed or cancelled export's incomplete checkpoint immediately, and recover crashed export captures from their saved intent at launch.
- 406ce3b: Record the new import's checkpoint group before loading it, so relaunch recovery can remove unfinished native checkpoints even when no sandbox settings were saved.
- 8ba2e0a: An unexpected internal error during one remote-computer, port-forwarding or GitHub operation no longer leaves those settings unavailable, or updates blocked, until Silo restarts.
- 5e4cbc8: A temporary failure to read VM state no longer replaces the loaded app with a full-window "Silo could not load" error. Silo keeps showing your VMs, marked as out of date with the error, until the next successful refresh.
- 668cc65: Retrying Start after a restored or forked sandbox ran but could not be verified now keeps that sandbox and starts it, instead of recreating it from the checkpoint and silently discarding changes made to its workspace.
- 3d7d411: Exports and imports of very large sandboxes on slow disks are no longer stopped after one hour; the time allowed now grows with the size of the sandbox's disks or the imported data.
- 8d8201f: Settings now suggest only terminals and code editors that can open sandboxes, and the system default falls back to one that can (for example Visual Studio Code instead of Xcode on macOS). Zed Nightly now opens sandbox folders too. On Linux, Open terminal works out of the box through the system's terminal launcher, Ptyxis and other launchers are supported, and Visual Studio Code from Microsoft's package, snap or Flatpak and Zed from its own installer or Flatpak open sandbox folders instead of failing or closing after a few seconds. When Silo runs as an AppImage, the terminals, editors and browsers it opens no longer inherit the AppImage's own libraries and settings.
- 6872e33: Linux guest LCU setup now registers native desktop access for its managed MCP server without changing the bundled LCU executable or its default behavior elsewhere.
- 6872e33: Linux packages now include WebKitGTK's H.264 decoder support for remote desktop video.
- 7d66cfb: Show firmware and nested virtualization guidance when Linux KVM reports unavailable hardware during its API check.
- d92c946: Linux packages now install the OpenSSH client, which the desktop viewer and editor handoff need.
- 7bf080c: Keep SFTP and editor uploads owned by the sandbox working account, so editors can install remote extensions and rename uploaded files.
- 8b44be3: On Linux, updating Silo no longer fails because another computer is managing this one or an editor is connected to a remote sandbox. When a running Silo or local VM does block an update, the error names that process.
- 46f84fa: A checkpoint that is still running no longer appears as interrupted with Retry, and clicking Start twice no longer reports a failed action.
- d654e4b: Restore live loopback port publishing and guest service reachability checks in the Network panel with the bundled MicroSandbox runtime. Removing a publication closes its active connections and releases its local port immediately.
- 854fdaa: Open help and menu links with the system environment when running the Linux AppImage.
- 4457c5d: Keep runtime diagnostics available behind Details in lifecycle failure notifications and Activity after separating them from error summaries.
- 372cea1: Upgrade the bundled VM runtime to MicroSandbox 0.7.4. Requests that use a secret placeholder in a header are no longer blocked when the request body contains percent or unicode escapes, and shared-folder files that keep a second hard link stay writable on macOS. Exports made by a Silo version that bundled MicroSandbox 0.7.2 can still be imported.
- 57faf4e: Allow workspace disk migration for crashed sandboxes without starting them, preserving the original disk and sandbox state.
- 4a355e4: Upgrading sandboxes to the checkpoint runtime no longer leaves an unused copy of each workspace disk in Silo's storage. On Linux that copy could take as much space as the workspace itself, and the upgrade now needs less free space while it runs.
- 6c93096: Explain native checkpoint, migration, account, desktop, and storage failures with a next step instead of internal worker names or exit codes.
- 1fc7716: Remove the artificial background from the application. Use native Liquid Glass on macOS 26 and desktop vibrancy on earlier supported macOS versions, with opaque accessibility and Linux fallbacks.
  
  When Reduce Transparency is enabled, restore the opaque theme colors for navigation, toolbar, and content in both light and dark mode.
- 41a734a: Use Export, Import, export file, sandbox, and checkpoint consistently in native transfer progress and errors.
- 08ab4cc: Label native menu and tray actions Open Silo consistently, without an ellipsis when they open the window directly.
- daa8e5a: Preserve the sandbox account returned by native SSH access state when displaying connection commands.
- 6076abf: Keep the macOS close, minimize, and zoom buttons centered in the toolbar after redraws, focus changes, and resizing.
- f8350f2: Reading and changing ports of one sandbox no longer waits while another sandbox's port forwarding is being updated or its runtime is slow to answer.
- 440fc0c: Ports and remote computer settings keep working after an internal error instead of failing until Silo restarts, and a saved port that could not be applied to a running sandbox now shows why on its row.
- d6ec2f1: Restore neutral light and dark backgrounds and keep the native sidebar, title bar, and status panel in sync with the selected theme, including System mode.
- 5660a47: Notifications now follow one rule: results with an action or from long operations stay until closed, quick confirmations disappear after a few seconds. Removing a port asks for confirmation in a popover, forwarded ports show both the VM port and local address, and a finished export or import from a previous session no longer reappears after relaunch.
- 8bd42e6: Status, SSH access, and port views stay responsive while a long operation such as a backup is running: reading them no longer waits its turn behind VM-changing work, and opening a remote SSH connection or browsing files is never queued. Port forwarding and SSH listener repairs still take their turn in order, so changes remain safely serialized.
- f22386e: Setup no longer deletes existing VMs when it starts before their configuration has loaded; it stops with an explanation and leaves every VM unchanged. Cancelling Quit (for example when a VM would not stop) no longer leaves setup and sandbox create, edit, or delete refusing with "Silo is quitting" until relaunch.
- 29471cd: Polish the sandbox detail page so it matches the rest of the app. Opening a sandbox no longer shifts the layout: the breadcrumb "Sandboxes" sits exactly where the list heading was, followed by the sandbox name. The header shows a compact status line (state, VM or computer, resources, restart-required, SSH) with labeled Terminal, Editor, Start/Stop, and More actions buttons. Tabs use the flush, underlined style, and the Overview, Checkpoints, Storage, and Access tabs now render as consistent labelled sections with bordered rows.
- 50f2632: Show the computer name for remote sandboxes in the port form's sandbox selector.
- 28fcb3e: While the sandbox storage upgrade is waiting, running or failed, Silo no longer touches your previous sandbox storage. Before, launching the new version could change that folder's database so that the older Silo could not open it again, and could interfere with the copy the upgrade was making. That folder now stays exactly as it was until the upgrade completes, and after you choose to continue without unmigrated sandboxes. Quitting and installing an update still work while a migration needs attention.
- adf8c8c: A finished operation no longer keeps showing as running when its change arrived while Silo was already reading the operation queue; network services refresh the same way.
- 5ce1770: A sandbox action that is still waiting its turn now shows what it is waiting for ("Waiting for …") instead of prematurely reading "Starting…" or "Stopping…", switching to the action label once it actually runs. Cancelling an action you asked to cancel is shown as a neutral "Stop cancelled" state with a Retry, not a red error, and clicking Retry clears the previous message right away. Quit no longer stalls behind long-running work: it cancels anything still queued, tells you what it is waiting for, and offers "Cancel and quit" to stop cancellable running work (non-cancellable work such as an update install is still waited for).
- 9f0a8c9: An export or import that waits for other sandbox work now shows "Waiting for other sandbox work" instead of claiming it is already creating snapshots. A checkpoint export keeps its "Exporting checkpoint" title after Silo reloads, and trying to start another export while one runs no longer renames the running export or changes its Retry.
- 24398ab: Background housekeeping no longer flashes a status above the sandbox list. Internal
  maintenance such as clearing expired logs, reconciling SSH access, trimming storage, and
  reconciling ports still runs under the same mutual-exclusion gate, but it is now hidden from
  the operation queue so it never shifts the list. Longer-running operations you started now
  appear as a single notification instead of an inline panel: it waits until an operation has
  been active briefly (so quick actions never flash), shows the running label or a count with
  elapsed time, flags anything taking longer than expected, lists what is waiting, and offers
  Cancel for a cancellable operation. Export and import keep their own progress notification. A
  sandbox waiting behind hidden maintenance now reads "Waiting for background maintenance…".
- ef9438e: Fix the Linux desktop viewer's Selkies status after the first frame is decoded.
- 5546137: Fix unexpected Paste menus when clicking the Linux desktop viewer by disabling unsupported automatic clipboard synchronization. Text transfer remains available through the viewer's clipboard panel.
- 9086cc7: Complete agent tool setup when adding a Linux desktop. Show a compact repair prompt only for detected installation problems, and put desktop Stop and Restart actions in the ellipsis dropdown.
- a573107: GitHub notifications now appear only for changes you make, and unchanged sandboxes are no longer re-applied when another sandbox retries. Notification close buttons are a clear × inside the top-right corner, and more notifications stay visible at once.
- ca6df97: Show a centered spinner over a softly blurred window while quitting and stopping local sandboxes, without shifting the app layout.
- ad2d095: Recover disk staging left by an interrupted import at the next launch, while preserving data from unrelated operations.
- d654e4b: Recover Linux desktops when a restored guest contains stale X11 socket files, while preserving active desktop sessions.
- 94d2c61: Inset sidebar submenu guides vertically and add space between the guides and submenu icons.
- bb99537: Use adaptive Regular glass on macOS 26 to improve readability over busy backgrounds. Make main content fully opaque in both themes while retaining glass in the sidebar and toolbar. Remove the extra teal overlay from native navigation and toolbar surfaces so the system glass material supplies their appearance.
- 5107df8: Include the H.264 GStreamer plugin when building Linux AppImages.
- 6046f78: Other computers can keep managing this one after Silo restarts, updates or runs from an AppImage: Silo re-points its remote bridge at the AppImage file (not its temporary mount) on every launch. On macOS, Silo asks to be moved to Applications instead of linking a temporary copy.
- a726c0f: Connecting to an address whose computer is already saved under another address now asks before replacing the saved address, so a copied or impersonating computer cannot silently take over another computer's connection. A computer whose Silo settings were copied from this one is now named as such, with steps to give it its own identity, instead of being reported as this computer.
- 1f3f137: Failed connections to another computer now say why: a changed or untrusted host key, failed SSH authentication, a refused or timed-out connection, an unknown address, Silo not running on the other computer, or a missing remote bridge.
- d654e4b: Fix remote VM desktop connections that could stall while opening private SSH by sending the connection response and binary guest traffic promptly.
- 2d6a5ec: Saving or deleting a sandbox on another computer no longer silently overwrites changes made there after you started editing; Silo now reports the conflict instead.
- c875e0d: "Set up Silo SSH key" now works when your account on the other computer uses fish, csh or another non-POSIX login shell.
- 622d640: Settings → Computers now lists every address another computer can use to reach this one, such as its `.local` name, Tailscale address and local network addresses, each with its own Copy button, instead of a single host name that often does not resolve elsewhere.
- 6046f78: Silo now opens even when remote management cannot start, for example when another Silo process owns it, its settings are damaged, or a different file is at `~/.local/bin/silo-remote`. Settings → Computers shows the reason, and turning remote management on again retries.
- 8ae5354: Removing a port of a sandbox on another computer now stops publishing it on that computer too, like removing a local port, instead of only closing this computer's connection. Saving a remote port or removing a computer no longer freezes the window, and a port that fails to move to a new local port keeps its working connection.
- 0143816: Command palette actions and a fork's Open button now act on the right sandbox when a remote computer has a sandbox with the same name as a local one, and palette entries name the remote computer. A remote sandbox's page no longer lists a local sandbox's secrets.
- 41ed208: Connecting to another computer whose shell startup files print text, such as an `echo` in `.bashrc` or conda's startup message, now works instead of failing with "Invalid remote Silo response".
- 7aafa03: Silo now connects to another computer with its own SSH key alone first, so an SSH agent holding many keys (1Password, Secretive) no longer exhausts the other computer's login attempts before Silo's key is tried. If only your own keys work there, Silo falls back to them and remembers that choice.
- ad7fb51: "Authorize SSH in Terminal…" and "Set up Silo SSH key…" no longer freeze the Silo window while Silo creates its key and opens the terminal.
- 25a55c5: Opening a terminal for a VM on another computer no longer edits `~/.ssh/config`, so it works when that file is a symlink.
- 8ae5354: Ports and desktop viewers opened from another computer no longer close on a single failed refresh, such as a network blip or a busy computer; they close only after repeated failures or when that computer no longer accepts this one. Port connections that dropped (for example after sleep) or whose sandbox restarted reopen by themselves on the same local port, and saving a port again with Automatic keeps its previous local port. While a computer is unreachable, Silo stops opening a new SSH connection for its network status on every refresh.
- 498cfe2: When starting, stopping, or restarting a VM on another computer fails, the error now appears on that VM with Retry, instead of every VM on that computer briefly showing "Computer unavailable" and the real error being lost.
- 142f6c2: Deleting a sandbox and adding a new one with the same name in one change now works, and the new sandbox never inherits the removed one's GitHub access.
- 6fa333f: Remove the redundant outer rounded border around Checkpoints inside the sandbox card. Keep checkpoint history, creation, restore, and fork actions available from the sandbox menu, and shorten the checkpoint creation button label.
- e7cda15: The Restore confirmation now explains that a running sandbox is paused, gets a recovery checkpoint that includes its memory, and is force-stopped, and that changes made outside the sandbox are not undone.
- a5b96a0: An interrupted or failed checkpoint Restore no longer leaves a sandbox stuck. If pausing the sandbox fails before its recovery checkpoint exists, the Restore is abandoned and the sandbox can be started and stopped again. If the sandbox stopped or crashed while its recovery checkpoint was being captured, retrying the Restore now saves a disk recovery checkpoint and finishes. Restore failures now show their real cause instead of "Silo closed during this operation", a failed save of checkpoint history no longer hides the original error, and a temporary runtime error no longer drops an interrupted checkpoint from the history.
- 1456180: After a fork or Restore Start creates its VM, Silo now reads that sandbox from the runtime instead of failing to load every sandbox when the start could not be verified. A fork or restored sandbox that started successfully is also recognized by the Storage panel and automatic space reclamation, instead of asking you to restart it.
- 5489a63: Silo's remote-management SSH key can now only run Silo's connection bridge and open tunnels to ports on the other computer's loopback address; it no longer grants a shell. Computers set up with an earlier version are tightened automatically the next time Silo connects to them.
- 5bc19b8: Disable failed-push retries when the sandbox is stopped, busy, or its status is unavailable.
- d6418ee: Open sandbox websites at their own localhost names in every browser, including Safari, so cookies stay separate between sandboxes. Keep the copied 127.0.0.1 address available for servers that reject other host names.
- c3a91b6: Update bundled help and website copy to explain the sandbox page, checkpoints, SSH access, and Export and Import with current control names.
- a398e1f: Switching to another sandbox's page now clears the previous sandbox's edit draft and delete confirmation, so an action on the page always applies to the sandbox shown.
- fb2fbb1: An open sandbox page no longer checks its ports, including on other computers, every few seconds while another section of Silo is shown.
- d2498ec: Show checkpoint progress once in the sandbox row and keep controls in place while operations run. Describe delayed remote status reads accurately and retry brief status-read collisions before showing a busy state.
- 22a70d1: Controls inside a sandbox row, such as Cancel and Dismiss, no longer also open the sandbox page, and screen readers now hear the row's status alongside its Open button.
- 15e2d93: Saving, removing or retrying a secret no longer makes forks fail with "Secret settings are busy" while a sandbox is still applying the change.
- 2dc3cef: A secret no longer shows "Restart to apply" for a sandbox that finished restarting while the change was being applied.
- 0e75fbf: Sandboxes with secrets start again after you unlock the system credential store, without first editing or retrying a secret.
- 85bb679: Keep fresher sandbox readings while another state read is settling, and accept completed readings without rolling back other sandboxes.
- 1a8899f: Show sandbox setup failures with expandable, copyable diagnostics in the status bar and sandbox list, and retain diagnostics in setup activity.
- 8abb00e: Show failed sandbox actions immediately in Overview and retain their diagnostic details in Activity. Include structured boot failures in Runtime logs, search, and export, even when the sandbox returns to Stopped.
- 62313aa: After an upgrade, Silo now tells you what became of an export or import that was interrupted before it, and when an export or import record it could not read was set aside. The result appears on the "Your sandboxes were updated" screen, and Open Silo marks it as seen; when that screen does not appear, it shows as an ordinary export and import notification that stays until you dismiss it. Outside an upgrade, an export or import record that is damaged or that Silo can't use no longer keeps exports and imports unavailable until you remove the file by hand: Silo renames it aside in its data folder, never deletes it, and tells you to run the export or import again if one was running. A record Silo can't open at all, for example because of a permissions problem, is left where it is and tried again at the next launch.
- Keep every sandbox’s completed stop in Activity when Quit stops sandboxes concurrently.
- 1a8899f: Stop local sandboxes concurrently during Quit and system shutdown so one slow sandbox does not delay the others.
- ff34fca: The SSH panel now shows and copies a working command such as `ssh -p 2222 silo@192.168.1.42` with the sandbox's real login account instead of an invalid `root@host:port` address. Sandboxes on another computer no longer show that computer's local-only `127.0.0.1` address.
- 0c982d4: Show and copy SSH addresses with the username included, such as `root@192.168.1.42:2222`, instead of a separate user label.
- 54d38f1: Set the sandbox login identity for SSH commands so shell tools receive the correct USER and LOGNAME variables.
- 683b763: Silo now asks before SSH access becomes reachable from other computers, including when SSH is turned back on while it is still set to allow other computers, and network access can be turned off while SSH is off. The SSH badge on sandbox rows and pages shows errors and unavailable status directly instead of only in its tooltip.
- b36d96b: SSH access status no longer freezes while an SSH listener starts, and Silo's background SSH check runs every 15 seconds instead of every 2 seconds, only when SSH access is turned on for a sandbox, so it no longer delays other sandbox actions.
- 19b6902: Keep successfully started sandboxes running when a follow-up state read or secret status update fails, and show a warning.
- 0759bd8: Sandbox states keep refreshing while Silo does background housekeeping or works on a different sandbox.
- c9a4307: "Open site" in the menu bar status panel now opens services on VMs running on another computer instead of reporting that the service is not reachable.
- 70c4edf: Stopped sandboxes no longer show errors on the GitHub, Network, and Logs pages, and sandboxes waiting to be restored show their saved settings. Terminal and editor actions now say "Start <name> first." Computers running an older Silo show an update hint for their logs instead of raw errors.
- 032a074: The Storage tab now measures a workspace disk that a checkpoint split into layers, including a sandbox restored or forked from a checkpoint, instead of showing 0 B or refusing to reclaim space. A disk Silo cannot find shows as Unknown.
- fd8cd8e: The Storage tab of a sandbox on another computer now names that computer instead of saying "This computer".
- 8c3d73a: The Storage tab now explains each measurement and the automatic reclaim schedule in visible text instead of hover-only tooltips, and a reclaim's Details button opens its error or result inline.
- b4ec0c0: Sandbox setting changes made while another operation is running now apply to the latest settings instead of overwriting concurrent work. Each create, edit, delete, or reorder is sent as a targeted change; if the sandbox changed while your edit was waiting its turn, Silo rejects it with a clear message so you can review and try again.
- 5052390: Sandbox and checkpoint menus no longer show captions under Fork and Duplicate settings. When Delete offers Export, then delete, its three buttons now stack full width instead of wrapping unevenly.
- 5512745: When an export file already exists, the error now says a file already exists at that path instead of describing it as a VM.
- 566a656: Exports and imports now check free space before copying data. An export stops before writing the file when the destination is too full, and an import stops before unpacking when Silo's working or runtime storage is too full. Both messages say how much space is needed and how much is available.
- 348b5f9: An export or import interrupted before a Silo runtime migration is now settled at launch instead of waiting forever, so it no longer tells you to relaunch to retry an operation Silo never attempted, and Retry in the migration screen can continue.
- 7529f05: An internal error during an export or import now ends that operation with a failure instead of leaving it running until Silo is relaunched.
- f1567e6: Copy published website addresses with the sandbox's own host name from the status bar.
- bff21d3: The menu bar sandbox menu now starts, opens and serves sites for the sandbox on the right computer, and only lists ports that can be opened.
- 0d3802e: Keep sandbox update, cancellation, and unsupported remote feature handling consistent when error messages change. Preserve cancellation outcomes across connected computers.
- bbc1b34: An unfinished Restore now shows in the sandbox's Checkpoints tab with the checkpoint it was restoring, and can be retried or abandoned there; Start and Stop explain the unfinished Restore instead of describing a fork. A sandbox a failed Restore left paused is resumed, or stopped if it cannot resume, and Quit no longer fails on it.
- f10c111: App updates are no longer blocked by a fork, restore, or import that has not been started yet, or by a sandbox that was deleted after an earlier update could not resume it.
- abf88f1: Upgrade Linux desktop agent tools to Luda 0.3.4, improving verification of GUI changes and incomplete-task reporting. Existing desktops receive the update through Set up agent tools or Repair agent tools.
- 9782350: Upgrade Linux desktop agent tools to Luda 0.3.1 for reliable application discovery, table selection, and clearer task verification. Configure XFCE desktop identity and include light and dark application themes. Existing desktops can upgrade through agent-tool setup; restart the desktop and reconnect agents afterward.
- da6c6ff: Upgrade Linux desktop agent tools to Luda 0.3.3, which explains selection-only verification directly in successful tool results and guides agents to verify the requested application effect.
- 6509ff1: Upgrade Linux desktop agent tools to Luda 0.3.2, with guidance that distinguishes selecting an item from applying or opening it. Existing desktops receive the updated skill through agent-tool setup or repair.
- f9acc16: Editing a sandbox that changed elsewhere now warns instead of overwriting. Silo remembers the sandbox's configuration from the moment you opened the editor (or started a delete or drag) and saves against that baseline, so a queued edit can no longer silently overwrite a concurrent change. If the sandbox changed while your edit was waiting, the editor stays open with your edits and offers to review the latest values or discard your changes; if it changed or was deleted while the editor was open, an inline notice tells you before you save.
- 1c54df7: A settings file written by a newer Silo, or one that cannot be read, no longer blocks Quit, app updates, or opening a terminal, editor, or browser. Silo leaves the file unchanged and uses the current settings for the session.

## 0.9.0

### Minor Changes

- 38f096a: Install Luda's desktop-control tools and skill for all supported agents when adding the Linux desktop, including agents installed later. Add setup and repair actions for existing desktops.
- 38f096a: Require one `silo` account for terminals, SSH, editors, files, and the Linux desktop. Older VMs require explicit migration or recreation instead of falling back to root. Before starting an older VM, close Silo and follow the [migration instructions](https://github.com/0xpolarzero/silo/blob/v0.9.0/docs/SiloUI-WORKING-ACCOUNT-MIGRATION.md), or recreate the VM. New VMs already use this account.

  Correct the release version to 0.9.0. The identical application changes were previously published as 1.0.0 in error; that release is superseded. Existing 1.0.0 installations require manual installation of 0.9.0.

## 0.8.0

### Minor Changes

- a9827c2: New VMs use the same Linux account for terminals, SSH, editors, file transfers, and the optional desktop, with passwordless sudo for administration. Existing VMs retain their accounts. Account and file-transfer tools are bundled in the guest image, so creating the working account needs no package downloads. Optional desktop installation still requires downloads. For new VMs on a remote computer, update Silo on both computers before using terminal, editor, or SSH access.

## 0.7.2

### Patch Changes

- Fix storage measurements and manual space reclamation being blocked by desktop permissions.

## 0.7.1

### Minor Changes

- 7e7fb3e: Add an optional interactive Linux desktop to new or existing sandboxes, with automatic or manual startup and a dedicated desktop viewer. Closing the viewer keeps graphical applications running.
- 2e6d920: Automatically reclaim unused local workspace disk space after seven days and before stopping when the last reclaim was at least a day ago, with bounded attempts that do not prevent shutdown. Add a Storage panel showing host disk allocation, workspace usage, manual reclamation, and a collapsed history of the latest 50 attempts, with measurement tooltips and reclaim progress. Fix a runtime bug that shortened disk images when reclaiming their unused tail.
- 6d5467c: Search all retained sandbox logs, browse older pages, filter by time and source, follow new output, inspect surrounding records, and export matching diagnostics with timestamps and computer, sandbox, source, and session details. Failed activities link to their diagnostic time window; unavailable computers show explicit errors.

### Patch Changes

- 2e6d920: Add Linux desktop directly from a sandbox's more-actions menu, with automatic startup enabled by default.
- 7acd6e7: Distinguish local and remote VMs with monitor and server icons, keeping the remote computer name in a neutral badge. Show one SSH badge: neutral for host-only access and blue with a network icon for access from other computers. Keep address copying in the expanded SSH controls.
- 020a8e0: Keep the Logs view focused on records and actionable errors by removing retention and search-scope commentary.
- 6b32d76: Distinguish local and remote VMs with monitor icons and a network marker, and local and network SSH access with server icons and the existing top-right network marker pattern. Keep SSH badge text compact, with scope explained in tooltips. Show the host computer name on remote VM rows, wrap connection labels and actions in narrow windows, and keep SSH settings expandable in the read-only website demo.
- e8c507c: Add a shortcut at the end of GitHub repository search results to authorize more repositories, and automatically load them when returning from GitHub.
- e8c507c: Clear GitHub settings progress when a newer settings revision completes with the same result as the previous save.
- 589702d: Add a refresh button before the Repositories caret in Files. Manual refresh bypasses cached repository scans on local and connected computers and shows progress while loading.
- 4d864a9: Match remote computer badges to local VM badges with the same rounded, borderless muted appearance.
- 77d34d1: Allow the main desktop window to search retained logs and export or cancel log exports.
- 7e7fb3e: Route Quit Silo on macOS through the shutdown flow so local sandboxes stop and pending settings save before the app exits.
- e5807fe: Make source and date filters optional, with removable chips and a Clear action. Simplify the logs empty state and show date controls only when adding or editing a date filter.
- 2e6d920: Group sandbox menu actions with desktop access, Restart and Storage first, followed by configuration actions and Delete.
- 12696cd: Use plain Search logs, Copy, and Export labels, with tooltips explaining what is copied or saved.
- 589702d: Refresh repository rows automatically while Silo is visible, so changes inside local VMs appear without switching windows. Avoid overlapping periodic reads when a scan is slow.
- 777e109: Remove the start-VM caption from waiting ports in the Network view.
- 72e4bb8: Retain runtime, execution, and kernel logs together for up to seven days within a 250 MiB sandbox budget. Rotate daily or at 10 MiB, remove the oldest segments first, and apply the same limits to kernel output and stopped sandboxes.
- 0a2e60c: Keep sandbox controls beside the name and status when there is room, instead of forcing them onto another line in moderately narrow windows.
- 6b32d76: Keep sidebar icons at the same position and size when expanding or collapsing the sidebar. Use compact circular backgrounds behind network corner markers so they remain legible without obscuring the VM or SSH icon.

## 0.7.0

### Minor Changes

- 7e7fb3e: Add an optional interactive Linux desktop to new or existing sandboxes, with automatic or manual startup and a dedicated desktop viewer. Closing the viewer keeps graphical applications running.
- 2e6d920: Automatically reclaim unused local workspace disk space after seven days and before stopping when the last reclaim was at least a day ago, with bounded attempts that do not prevent shutdown. Add a Storage panel showing host disk allocation, workspace usage, manual reclamation, and a collapsed history of the latest 50 attempts, with measurement tooltips and reclaim progress. Fix a runtime bug that shortened disk images when reclaiming their unused tail.
- 6d5467c: Search all retained sandbox logs, browse older pages, filter by time and source, follow new output, inspect surrounding records, and export matching diagnostics with timestamps and computer, sandbox, source, and session details. Failed activities link to their diagnostic time window; unavailable computers show explicit errors.

### Patch Changes

- 2e6d920: Add Linux desktop directly from a sandbox's more-actions menu, with automatic startup enabled by default.
- 7acd6e7: Distinguish local and remote VMs with monitor and server icons, keeping the remote computer name in a neutral badge. Show one SSH badge: neutral for host-only access and blue with a network icon for access from other computers. Keep address copying in the expanded SSH controls.
- 020a8e0: Keep the Logs view focused on records and actionable errors by removing retention and search-scope commentary.
- 6b32d76: Distinguish local and remote VMs with monitor icons and a network marker, and local and network SSH access with server icons and the existing top-right network marker pattern. Keep SSH badge text compact, with scope explained in tooltips. Show the host computer name on remote VM rows, wrap connection labels and actions in narrow windows, and keep SSH settings expandable in the read-only website demo.
- e8c507c: Add a shortcut at the end of GitHub repository search results to authorize more repositories, and automatically load them when returning from GitHub.
- e8c507c: Clear GitHub settings progress when a newer settings revision completes with the same result as the previous save.
- 589702d: Add a refresh button before the Repositories caret in Files. Manual refresh bypasses cached repository scans on local and connected computers and shows progress while loading.
- 4d864a9: Match remote computer badges to local VM badges with the same rounded, borderless muted appearance.
- 77d34d1: Allow the main desktop window to search retained logs and export or cancel log exports.
- 7e7fb3e: Route Quit Silo on macOS through the shutdown flow so local sandboxes stop and pending settings save before the app exits.
- e5807fe: Make source and date filters optional, with removable chips and a Clear action. Simplify the logs empty state and show date controls only when adding or editing a date filter.
- 2e6d920: Group sandbox menu actions with desktop access, Restart and Storage first, followed by configuration actions and Delete.
- 12696cd: Use plain Search logs, Copy, and Export labels, with tooltips explaining what is copied or saved.
- 589702d: Refresh repository rows automatically while Silo is visible, so changes inside local VMs appear without switching windows. Avoid overlapping periodic reads when a scan is slow.
- 777e109: Remove the start-VM caption from waiting ports in the Network view.
- 72e4bb8: Retain runtime, execution, and kernel logs together for up to seven days within a 250 MiB sandbox budget. Rotate daily or at 10 MiB, remove the oldest segments first, and apply the same limits to kernel output and stopped sandboxes.
- 0a2e60c: Keep sandbox controls beside the name and status when there is room, instead of forcing them onto another line in moderately narrow windows.
- 6b32d76: Keep sidebar icons at the same position and size when expanding or collapsing the sidebar. Use compact circular backgrounds behind network corner markers so they remain legible without obscuring the VM or SSH icon.

## 0.6.3

### Patch Changes

- 3eacd00: Use standard Git and Git LFS transfers for explicit pushes, including empty files and historical LFS data. Reuse a bounded publishing cache without giving sandboxes GitHub write access. Keep push progress and results across remote disconnections, prevent duplicate requests, and identify unknown outcomes after a host restart. Update Silo on both computers to use the new remote push flow.
- 298879d: Keep the status menu fully visible when sandbox content changes or loads, including the Open Silo and Quit controls.
- a1c0565: Place the smaller crash Dismiss button beside the sandbox error message.

## 0.6.2

### Patch Changes

- 826fae1: Allow updating and quitting when a sandbox has crashed. Add Dismiss to acknowledge a sandbox crash and show it as stopped without deleting its data or starting it; later crashes remain visible.

## 0.6.1

### Patch Changes

- 2cfc0f5: Show the failed object, process exit status, transferred bytes, and bounded runtime diagnostics when a repository push cannot copy committed objects from its sandbox. Report a file-size limit only when the transfer process receives the corresponding signal.
- 261c792: Keep normal HTTPS certificates for destinations that do not receive sandbox secrets, fixing TLS failures in SSH-connected coding agents that discard inherited CA settings. Secret destinations retain TLS interception, and live secret changes update which destinations are intercepted.
- b4de71a: Show the required SSH username beside each connection address and clear command copy feedback automatically instead of leaving “Command copied” in the menu. Place SSH controls before Start/Stop in sandbox rows.

## 0.6.0

### Minor Changes

- Open sandbox SSH access from the Overview page, with automatically generated connection keys, editable ports, copyable addresses, and optional LAN or VPN access. Local access remains available when network access is enabled.
- Manage SSH access on connected computers. SSH access stays within the sandbox’s running lifetime and closes when its owning Silo app exits.
- Keep sandbox controls compact: Terminal, Editor, Start/Stop, and SSH stay visible; Restart, Edit, Duplicate, and Delete move into an icon-labeled menu. SSH badges provide quick address copying.
- Show sandbox counts by computer location and simplify Activity cards, waiting-port messages, and GitHub settings.
- Keep update notices concise and place manual installation instructions in an expandable section while preserving automatic Debian installation.

## 0.5.2

### Patch Changes

- 5b65788: Align the macOS window controls with the sidebar and navigation buttons across macOS SDK versions.

## 0.5.1

### Patch Changes

- fba6ede: Update Debian installations directly in Silo with system authentication, package-list refresh, progress, and restart. Check for updates when returning to Silo, with throttling and offline retries. Older installations need this release installed once before the new Update action is available.

## 0.5.0

### Minor Changes

- a188454: Connect a personal GitHub token alongside GitHub OAuth and choose the connection for each VM. Tokens stay on the host, support accounts with no repositories, and provide their full GitHub permissions. Disconnected methods are unavailable without switching VMs to another connection. Existing VMs running an older Silo runtime need one restart before using a personal token.

### Patch Changes

- e2faa5d: Remove the redundant caption beneath network ports waiting for a service.

## 0.4.4

### Patch Changes

- Fix AI agents disconnecting after reading secret placeholders. Unmatched placeholders now pass unchanged, while real credentials remain restricted to allowed domains. Update Silo on every computer running your VMs, then restart those VMs to apply the fix.

## 0.4.3

### Patch Changes

- Fix AI agents losing their connection after reading secret placeholders. Requests to other domains now carry placeholders unchanged; real credentials are still substituted only for allowed domains. Update Silo on each computer that runs your VMs, then restart existing VMs to apply the fix.

## 0.4.2

### Patch Changes

- dc7b891: Include curl in newly created VMs. Existing VMs keep their installed packages; run `apt-get update && apt-get install -y curl` inside an existing VM if needed.

## 0.4.1

### Patch Changes

- Add a Linux Menu button with Alt/F10 access and Escape focus restoration. Find update checks, downloads, installation, retries, and installer links through Command-K or Control-K, with confirmation before stopping running sandboxes.

## 0.4.0

### Minor Changes

- Add a Linux application menu accessible from the Menu button, Alt, or F10. Escape dismisses the menu and returns keyboard focus to the application.

  Find update checks, downloads, installation, retries, and manual installer links through Command-K or Control-K. Available commands follow update progress and preserve confirmation before stopping running sandboxes.

## 0.3.3

### Patch Changes

- Check for updates shortly after launch, retry failed checks automatically, and check promptly after automatic checks are re-enabled or the computer resumes. Preserve pending updates and download retries during background checks.

  Clarify the Linux Software Updater instructions and label the GitHub installer link accurately. Prevent edits during installation so settings are saved before Silo restarts.

## 0.3.2

### Patch Changes

- Fix Cancel and Open browser again being blocked by desktop permissions during GitHub connection. Show connection errors in onboarding so failed recovery actions are visible.

- Add terminal and code editor buttons beside sandbox lifecycle controls in the overview. The editor button lets you choose a sandbox folder.

## 0.3.1

### Patch Changes

- Allow completing setup without a sandbox, deleting the last sandbox, and creating one later. Preserve intentionally empty setup drafts across relaunches.

  Allow Quit after first-time runtime setup failed before any VM was created, even when an older version left a directory in place of the runtime alias. Continue checking shutdown whenever runtime state or an active worker exists.
- Add Cancel and Open browser again while connecting GitHub. Cancel stops the pending authorization without disconnecting an existing account, and retrying cannot be overwritten by a cancelled attempt. Keep the existing Continue flow available during setup.

## 0.3.0

### Minor Changes

- 2409cc0: The Linux installer now offers updates through Software Updater. Enable Silo's signed software source once to receive future releases with your other application updates. Package upgrades ask you to quit Silo first so local VMs are not interrupted by replacing the application.

## 0.2.3

### Patch Changes

- Show available updates during setup, with update controls accessible without completing onboarding.
- Fix runtime initialization during sandbox recovery and secure Silo's shared directory when it already has group-write permissions. Report the conflicting path when a runtime alias is occupied, preserving existing data.

  Enable text selection in activity entries and add a button to copy each activity's title and details.

## 0.2.2

### Patch Changes

- 861a356: Correct Silo Help to explain CPU and memory edits, remote computer setup, and how closing or quitting affects running sandboxes.
- fde17c1: Choose smaller presets or custom whole-number values for VM CPUs, memory, and new VM disks. Custom resource settings persist across restarts, with limit, ceiling, and storage validation. Existing VM disks remain read-only.

## 0.2.1

### Patch Changes

- Manage another computer’s VMs over SSH from the Computers settings category. Connect to a computer running Silo to create, start, stop, edit, and delete its VMs, and open their terminals and files.

  Remote management requires Silo to remain running on the owning computer, remote management to be enabled, and SSH access to that account. Quitting Silo stops its local VMs and disconnects remote sessions; VMs owned by other computers keep running.

  This release includes the remote computer features from the unpublished 0.2.0 build.

## 0.2.0

### Minor Changes

- 9e7845d: Manage another computer’s VMs over SSH from the new Computers settings category. Connect to a computer running Silo to create, start, stop, edit, and delete its VMs, and open their terminals and files.

  Remote management requires Silo to remain running on the owning computer, remote management to be enabled, and SSH access to that account. Quitting Silo stops its local VMs and disconnects remote sessions; VMs owned by other computers keep running.

# V: end-to-end run with the app on screen

Run on the Dev build (`Silo Dev`) built from `direction`, against the Dev engine, with throwaway
`e2e-v-*` computers. Drive the app through LCU (menus and clicks; AZERTY keyboard, French macOS:
no letter shortcuts). Record PASS/FAIL per line in `docs/direction/evidence/V.txt` with what was
seen. Never touch the production app or `~/.silo`.

## Approvals (T10, R4)
- [x] Sidebar shows Pending with a count when an agent asks.
- [~] An agent in `e2e-v-main` asks to publish a port: toast with Review; system notification with
      Silo in the background; menu bar row "waiting for you"; clicking the notification opens it.
      Result: toast with Review passes; system notification, menu bar row and clicking the notification skipped (macOS permission banner not answerable through LCU, tray panel not reachable).
- [x] Allow it; the port works. Deny another with a note; the agent sees the note.
- [x] An agent asks to merge a fork into main: open it while another approval is open; the diff
      shows (R4 fix). Deny: the review request does not come back as a second row (R4 fix).
- [x] A merge with conflicts: choose per item, merge again.
- [x] Approval detail shows the computer id beside its name.
- [-] Settings > Notifications > "Requests from agents" toggles the notifications.
      Result: skipped: category switches are disabled while "Enable notifications" is off (waiting on the macOS permission prompt).
- [~] Activity > "Who did what": filters, Load more, Open request.
      Result: filters and Load more pass; Open request did nothing on a removed computer's row (FAIL, FINDINGS 8).

## Agents (T13a)
- [~] Onboarding with `main`: choose an agent, install works; a failing install shows Retry.
      Result: install works; a failing install shows its error and an Install button (not labelled Retry); the onboarding screen itself was not shown.
- [~] Open Claude Code and Open Codex from the header, the computer menu, the palette and the menu
      bar land in `/workspace` (`/Users/silo/workspace` on macOS).
      Result: header, computer menu and palette land in /workspace on Linux; menu bar skipped; macOS not run.
- [~] Agents section: Install and Update. Show and copy the SSH name.
      Result: Install and SSH name show/copy pass; Update skipped (nothing newer).
- [x] Create a computer with an agent selected.
- [-] Opening an agent on a macOS computer leaves `~/.ssh/config` unchanged (R4).
      Result: skipped: no macOS computer (disk at 92%).

## Network and resources (T13b, R4)
- [x] Change a computer's network mode, Save, Restart; the mode applies.
- [x] Add a host; an invalid one (`Example.com:80`, `*.com`) is refused as you add it.
- [-] A preset update: Save does not accept it; Accept shows the hosts it adds.
      Result: skipped: no preset change could be produced.
- [x] Two edits at once (app and `silo network` in a terminal): the second Save shows Read again.
- [x] Toggle Silo's tools for a computer; `silo tools status` inside agrees.
- [x] Network page links open the computer's section; create a computer with a mode.
- [x] Settings > Keep free: edit the floors; Enter then click away saves once.
- [x] A network request from an agent appears on Pending.

## Inside a computer (T5b, T6, R1, R5)
- [~] `silo list`, `silo tools status`, the MCP tool list in Claude Code inside the computer.
      Result: list and `claude mcp list` pass; `silo tools status` inside is refused (FAIL, FINDINGS 9).
- [x] `silo recipe show <own recipe>` hides commands and says so; another recipe is refused.
- [x] `silo fork`, `silo merge` back with an approval; undo it.

## Rebuild (T15), if merged
- [x] `silo rebuild e2e-v-linux --force` from the device; the app shows the step label; the
      computer keeps its workspace and home; `-undo` appears stopped.

Legend: [x] pass, [~] partly (see Result), [-] skipped. Full record: evidence/V.txt.

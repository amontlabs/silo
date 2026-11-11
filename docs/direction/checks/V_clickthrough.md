# V: end-to-end run with the app on screen

Run on the Dev build (`Silo Dev`) built from `direction`, against the Dev engine, with throwaway
`e2e-v-*` computers. Drive the app through LCU (menus and clicks; AZERTY keyboard, French macOS:
no letter shortcuts). Record PASS/FAIL per line in `docs/direction/evidence/V.txt` with what was
seen. Never touch the production app or `~/.silo`.

## Approvals (T10, R4)
- [ ] Sidebar shows Pending with a count when an agent asks.
- [ ] An agent in `e2e-v-main` asks to publish a port: toast with Review; system notification with
      Silo in the background; menu bar row "waiting for you"; clicking the notification opens it.
- [ ] Allow it; the port works. Deny another with a note; the agent sees the note.
- [ ] An agent asks to merge a fork into main: open it while another approval is open; the diff
      shows (R4 fix). Deny: the review request does not come back as a second row (R4 fix).
- [ ] A merge with conflicts: choose per item, merge again.
- [ ] Approval detail shows the computer id beside its name.
- [ ] Settings > Notifications > "Requests from agents" toggles the notifications.
- [ ] Activity > "Who did what": filters, Load more, Open request.

## Agents (T13a)
- [ ] Onboarding with `main`: choose an agent, install works; a failing install shows Retry.
- [ ] Open Claude Code and Open Codex from the header, the computer menu, the palette and the menu
      bar land in `/workspace` (`/Users/silo/workspace` on macOS).
- [ ] Agents section: Install and Update. Show and copy the SSH name.
- [ ] Create a computer with an agent selected.
- [ ] Opening an agent on a macOS computer leaves `~/.ssh/config` unchanged (R4).

## Network and resources (T13b, R4)
- [ ] Change a computer's network mode, Save, Restart; the mode applies.
- [ ] Add a host; an invalid one (`Example.com:80`, `*.com`) is refused as you add it.
- [ ] A preset update: Save does not accept it; Accept shows the hosts it adds.
- [ ] Two edits at once (app and `silo network` in a terminal): the second Save shows Read again.
- [ ] Toggle Silo's tools for a computer; `silo tools status` inside agrees.
- [ ] Network page links open the computer's section; create a computer with a mode.
- [ ] Settings > Keep free: edit the floors; Enter then click away saves once.
- [ ] A network request from an agent appears on Pending.

## Inside a computer (T5b, T6, R1, R5)
- [ ] `silo list`, `silo tools status`, the MCP tool list in Claude Code inside the computer.
- [ ] `silo recipe show <own recipe>` hides commands and says so; another recipe is refused.
- [ ] `silo fork`, `silo merge` back with an approval; undo it.

## Rebuild (T15), if merged
- [ ] `silo rebuild e2e-v-linux --force` from the device; the app shows the step label; the
      computer keeps its workspace and home; `-undo` appears stopped.

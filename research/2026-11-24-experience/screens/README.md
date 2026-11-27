# Screens drawn for the night case

Static pictures, drawn on 24 November 2026 for the owner to look at before anything is built.
Nothing here is a decision. The content is the night of [`daily-loop.md`](../details/daily-loop.md)
and the `silo` output of [`cli.md`](../details/cli.md). Each picture is an HTML file with a PNG of
it beside it.

Marks follow record-ux Part 2. A fact has no mark. ◌ is a report, told by the probe. ◇ is an
opinion. Words an agent wrote are between « and ». On the Mac, each mark has a tooltip of six
words or fewer. The light Mac picture shows one of them open.

| Picture | What it shows | Plan entries | Proposals in daily-loop.md |
|---|---|---|---|
| `phone-lock-ask.png` | The iPhone lock screen at 01:12 with one ask: a push to github.com/ana/app, branch main, 3 commits, held with 74 s left, and claude #2's words. Deny is one tap. Allow once has a lock, because it needs Face ID. | `what-an-ask-shows`, `ask-reaches-you`, `phone-app`, `hold-limit-length` | Gap 4: the lock-screen ask, Deny one tap, Allow once behind Face ID, time left shown |
| `phone-lock-folded.png` | The same lock screen when more than three asks arrive in a minute: one notification, "7 asks from main". The hosts in it are made up for the picture. | `ask-reaches-you`, `alert-only-on-a-change` | Gap 4: a flood is folded |
| `phone-list.png` | The phone app at 06:58, with Away on until 07:00. It lists the pushes refused while you were away, with Allow next time, and three items for the morning. There is no Asks section, because nothing is held while you are away. | `phone-app`, `one-queue`, `silo-proposes-rules` | Gap 4: refused asks stay, Away said by you, the phone app's first job. Gap 5: the morning list |
| `phone-list-day.png` | The same app at 14:20, with Away off. Two asks wait, with their time left, Deny and Allow once. One alert follows. The Stripe ask is made up for the picture. | `phone-app`, `one-queue`, `what-an-ask-shows` | Gap 4, item 3 |
| `mac-morning.png`, `mac-morning-dark.png` | The Mac app's first screen at 06:58, light and dark. One line counts what happened since Mon 22:58. Needs you holds the deleted folder with Put back from 02:46, codex stuck in try-bun, and the 2 refused pushes. Done holds claude #1, whose «All tests pass» is a mismatch, and claude #2. Now, Disk, Network and Timeline are at the bottom. | `first-screen`, `one-queue`, `kinds-look-different`, `how-each-run-is-going`, `restore-saves-first` | Morning: the Mac's sleep line, the refused asks. Gap 6: put back paths from a checkpoint |
| `mac-notification.png` | A macOS banner for the same ask, with Allow once and Deny, while claude #2 waits on `git push` in the terminal behind it. | `ask-reaches-you`, `held-requests-and-asks` | Gap 7: asks as Mac notifications |

The lock-screen ask is drawn at 01:12 as an example of a night when you are not away. On the night
of the morning screens you were away, so the pushes were refused and none reached the phone.

The drawn Mac banner shows no step-up on Allow once. This research's text now asks for Touch ID or
the Mac's password on Allow, as Face ID is asked on the phone ([`daily-loop.md`](../details/daily-loop.md),
gap 7).

To make the PNGs again, open each HTML file in Playwright's Chromium at 390×844 (phone) or
1280×800 (Mac), with a device scale factor of 2. `phone-lock-ask.html#folded` and
`phone-list.html#day` draw the second state of those files.

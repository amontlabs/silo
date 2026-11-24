
## 23. Xcode copied in from the Mac, and the iOS runtime downloaded in the computer

Does Xcode copied from this Mac into a computer pass Apple's signature check, accept its license and finish its first launch over SSH with nobody at the screen, and does the iOS simulator runtime download there with no Apple Account?

- Decides: xcode-in-a-computer, first-run-time-macos, computer-image (whether the kept image can hold Xcode)
- Tier: `vm`. Time: about an hour, most of it copying Xcode and downloading the runtime. Needs: the base computer of test 00 with at least 80 GB of disk, macOS 27, Xcode installed on this Mac, no sudo on the Mac.
- Script: [`tests/23-xcode-into-a-computer.sh`](tests/23-xcode-into-a-computer.sh)

| Outcome | What it means |
|---|---|
| `ready` | Xcode passed the signature check, its license and first launch were done over SSH, and the iOS runtime downloaded and shows as available. Bringing Xcode in from the Mac works with nobody clicking; the sizes and times are in the measures. The computer is kept as the Xcode computer for tests 24 to 27. |
| `signature-refused` | codesign or Gatekeeper refused the copied Xcode in the computer. A copy over SSH loses something (an extended attribute, a signature seal), so Silo must import the .xip instead and expand it inside. |
| `first-launch-failed` | Xcode passed the signature check but its license or first launch failed or waited on the screen over SSH. xcode-in-a-computer needs a step on the computer's screen, or another way to finish first launch; the sentence says which step. |
| `runtime-refused` | Xcode is ready but the runtime download failed in the computer (the error is in the sentence). If it asks for an account, xcode-in-a-computer must bring the runtime in from the Mac too. |
| `inconclusive` | something else went wrong; the sentence and logs say what. |

## 24. A simulator booted and used over SSH, with and without a user on the screen

Over SSH, does a simulator in a computer boot, install and launch an app, and give a screenshot that is not blank, first as the computer starts, then after its user is logged in on its screen?

- Decides: computers-macos-screen, computer-image (whether main logs its user in on its screen), simulators-for-parallel-agents
- Tier: `vm`. Time: about 25 minutes. Needs: the Xcode computer of test 23, macOS 27, no sudo on the Mac.
- Script: [`tests/24-simulator-over-ssh.sh`](tests/24-simulator-over-ssh.sh)

| Outcome | What it means |
|---|---|
| `works-without-screen-login` | Everything worked over SSH with nobody logged in on the computer's screen. Main needs no login on its screen for simulators; the boot time and the screenshot are in the measures. |
| `needs-screen-login` | It failed with nobody on the screen and worked once the user was logged in on it (automatic login, then a restart). Main must log its user in on its own screen at every start, and computer-image must set that up. |
| `already-on-screen` | The provisioned computer logged its user in on its screen by itself, and everything worked. Main gets the session simulators need with nothing to set up. |
| `blank-screenshot` | The app ran but every screenshot was blank (one colour). The guest's graphics do not render the simulator; agents cannot check their work by looking at it. |
| `fails` | It failed in both cases; the step and error are in the sentence. |
| `inconclusive` | something else went wrong; the sentence and logs say what. |

## 25. How many booted simulators fit in one computer

In a computer, how much memory does each booted simulator with an app running take, and how many boot before the computer starts to swap or a boot fails?

- Decides: simulators-for-parallel-agents (the maximum booted at once), sizing-target (memory of main for an iOS developer)
- Tier: `long`. Time: about 40 minutes. Needs: the Xcode computer of test 23 (with its user on the screen if test 24 said so), macOS 27, no sudo on the Mac.
- Script: [`tests/25-simulators-per-computer.sh`](tests/25-simulators-per-computer.sh)

| Outcome | What it means |
|---|---|
| `measured` | Simulators were booted one at a time until the computer swapped, a boot failed or the limit of the run was reached. The memory each one added and the number that fit are in the measures; the maximum of simulators-for-parallel-agents is set from them. |
| `first-boot-failed` | Not even one simulator booted; see test 24 first. |
| `inconclusive` | something else went wrong; the sentence and logs say what. |

## 26. codesign in a computer, over SSH, with no dialog

Over SSH, does codesign sign with an identity in the login keychain, and with one in a keychain of its own set up the way CI does it, without a dialog on the screen?

- Decides: apple-keys-in-a-computer (whether a development identity in the computer works for agents with nobody at the screen)
- Tier: `vm`. Time: about 15 minutes. Needs: the Xcode computer of test 23, macOS 27, no sudo on the Mac; no Apple account (a self-signed identity is made in the computer).
- Script: [`tests/26-codesign-over-ssh.sh`](tests/26-codesign-over-ssh.sh)

| Outcome | What it means |
|---|---|
| `both-sign` | Both keychains signed over SSH. Nothing special is needed for a development identity in a computer. |
| `own-keychain-only` | The login keychain failed or waited, and a keychain of its own, unlocked in the same session with its partition list set, signed. Silo's Apple development piece sets that keychain up. |
| `neither` | Neither signed; the errors are in the sentence. errSecInternalComponent means the keychain was locked; a time-out means a dialog waited on the screen. |
| `inconclusive` | something else went wrong; the sentence and logs say what. |

## 27. Which certificates an app in a simulator trusts

Does an app in a simulator trust a certificate authority that the computer's system trusts, and does it after `simctl keychain add-root-cert`?

- Decides: registry-traffic-read and secrets-held-outside (whether Silo's certificate must be added to every simulator), simulators-for-parallel-agents (a step for each simulator made)
- Tier: `vm`. Time: about 20 minutes. Needs: the Xcode computer of test 23 (with its user on the screen if test 24 said so), macOS 27, no sudo on the Mac.
- Script: [`tests/27-certificate-in-simulators.sh`](tests/27-certificate-in-simulators.sh)

| Outcome | What it means |
|---|---|
| `separate-store` | The app refused the server before, still refused it once the computer's system trusted the authority, and accepted it after add-root-cert. Every simulator Silo or an agent makes needs Silo's certificate added to it. |
| `shares-system-store` | The app accepted the server once the computer's system trusted the authority. Nothing more is needed for simulators. |
| `system-trust-refused` | Adding the authority to the computer's system store failed over SSH (as in test 21); the simulator step was still tried and its result is in the sentence. |
| `never-trusted` | The app refused the server even after add-root-cert. |
| `inconclusive` | something else went wrong; the sentence and logs say what. |

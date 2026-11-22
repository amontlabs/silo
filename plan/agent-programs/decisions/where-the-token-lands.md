# Where the token lands when each agent app connects over SSH

## The choice

An agent's part, 22 November 2026. You drive main from the Claude app and from the Codex app on your
Mac, both connected over SSH. Then you fork main to let an agent try a tool you do not trust.
Whether that fork can use your Claude or ChatGPT account depends on where each app left your login
in main, and nobody has checked yet.

The options:

- Checked for each app (recommended). A live check connects each app to a computer and signs in,
  looks for the login on the computer's disk and in the programs the app starts there, and forks the
  computer to see whether the fork's copy works. What is known so far: the Claude app appears to
  keep the login on your Mac and hand main a short-lived token, and the Codex app signs in on main
  itself, so its login lands on main's disk. It costs a check with a real account for each app,
  again whenever an app changes how it signs in.
- Not checked. Silo keeps logins out of forks only for programs you sign in to inside the computer,
  and says it cannot vouch for a computer you drove from an app. It costs that promise for a common
  way of working: from the app on your Mac.

Recommended: checked for each app. That a login does not end up in a fork by accident is a promise
you took in `login-handling`, and the apps are a common way in.

## What is known

- The Claude desktop app appears to keep the login on the Mac and to give the session a
  short-lived token.
- With Codex, the sign-in is done on the remote machine.

---
"silo-ui": minor
---

Apps that embed Silo's engine can now install Node.js 24.21.0 and the Claude Code CLI 2.1.287 in a Linux computer with the new `setup-agent-tools` desktop action, and read whether they are installed from the computer's status. Nothing is installed unless an app asks. Silo never signs in to Claude Code; the app that runs it provides the sign-in.

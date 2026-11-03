---
"silo-ui": minor
---

The engine protocol is now version 7: the ChatGPT app and LCU methods, events and fields are gone (`preparation.chatgpt.status`, `preparation.chatgpt.retry`, `preparation.chatgpt.changed`, `computers.creation.skipComputerUseWait`, `PreparationStatus.lcu`, and `cause`, `compatibility`, `warning`, `appVersion` and `runtimeVersion` in the computer-use state, which gains `driverVersion`). Clients on protocol 6 are refused at the handshake.

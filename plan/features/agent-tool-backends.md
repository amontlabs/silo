## Notes

- Mistral Vibe has an experimental `vibe -p --agent-socket PATH` mode: a JSON-RPC protocol (`agent/initialize`, `sandbox/execute`, `sandbox/readFile`, `tools/call`). A server for it would run Vibe's file and shell tools inside a computer, with per-repository GitHub access and no secrets in the computer.
- UK AISI's Inspect (`inspect_ai`) takes sandbox environment providers; follow the pattern of `inspect_proxmox_sandbox` and `inspect_k8s_sandbox`, as a separate repository, with a pull request to the Inspect extensions list. None of the listed providers runs macOS.
- OpenClaw and Hermes both document an SSH sandbox or terminal backend. A `silo ssh-config <computer>` command that prints or installs the SSH entry, plus a step-by-step page per project following its own SSH-backend docs, is the cheapest integration. Linux computer by default; macOS only when the agent needs iMessage.
- Docs describe the mechanism only, without "secure" or "safe".

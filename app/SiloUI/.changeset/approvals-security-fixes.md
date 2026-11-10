---
"silo-ui": patch
---

Requests from the agent inside a computer are safer to read and answer. What the agent reads back after you allow a request is only what concerns that request, never the device's secrets, GitHub accounts or other computers. Each request now says what will really run: the repository, branch and commit of a push, the address and keys of SSH access, the scheme of a published port, the commands and database choices of a merge, and what a computer's GitHub access was and will be. A secret's value is deleted from Silo's files as soon as the request is answered. A computer can ask for at most 30 requests an hour. The agent's words are shown without control characters, and replacing main or undoing a merge is refused if things changed after the agent asked.

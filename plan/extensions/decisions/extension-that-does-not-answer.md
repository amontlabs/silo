# What wakes an extension, and who chooses what happens when it does not answer in time

## For the owner

An agent's part, 22 November 2026. An extension you installed to deny unsafe requests is slow or
stuck, and a request from main waits for its answer. Today the decision you took gives the choice to
the extension's author.

The options:

- Author's default, you can change (recommended). The extension says whether the request is refused
  or left to your rules. You see it when you install the extension, and you can change it in Silo.
  It costs one more setting on each extension that guards requests.
- The author decides. What the extension's manifest says holds, as shown when you installed it. It
  costs your say, since you have no setting.
- Always refused. A request whose guard did not answer is refused. It lets nothing past a silent
  guard, and one slow extension can then block your agents.

Recommended: author's default, you can change. You are the one who pays for blocked work or for a
missed refusal, so the last word is yours.

This would replace `what-wakes-an-extension` and keeps the rest of it word for word. What a
precaution does with the answer "unsure" stays in `precaution-on-unsure`. The audit's finding is in
the notes of `what-wakes-an-extension`.

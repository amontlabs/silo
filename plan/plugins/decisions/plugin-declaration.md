# A plugin is a program with a declaration

The owner said this. That the user accepts the declaration and Silo enforces it afterwards is put
forward on its own: `declaration-is-enforced`. The standard name of a declaration is a manifest.

| The declaration says | Field | Example |
|---|---|---|
| what makes it run | `on` | a tool call waiting at the boundary; another plugin's opinion |
| its input | `inputs` | what the user asked the agent, and the tool call |
| its output | `outputs` | an opinion: safe, not safe or unsure |
| how Silo starts it | `runs` | in Silo when it is data; in a computer of its own when it is code or has tools |
| the rules it needs | `permissions` | read the conversations of main |
| the precautions it comes with | `precautions` | "on not safe: deny" |

The owner, on 20 and 21 November 2026: a declaration uses the standard names, not plain labels
such as "given" or "gives back". The owner called the field names above perfect. They are the ones
GitHub Actions uses. What goes under each field is settled later; the owner is not sure what goes
under `runs`.

The rules a plugin needs are shown when it is imported and switched on when it is installed:
`import-then-install`, and `plugin-calls`.

The owner's list for the interface of an added piece: what it receives, what it can fetch from
Silo, how it hooks in, how it sends things outside or into the app, and extra tools it brings.

The owner's word "triggered" in the answer is the one `plugin-wakes-on` would replace for a plugin.

An older text called a plugin a package through which pieces made by someone else arrive. That is
not carried: what it brings is now said by the declaration.

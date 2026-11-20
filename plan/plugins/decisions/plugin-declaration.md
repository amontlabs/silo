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

## The owner's answers (21 November 2026)

- `runs` is important. The example shown had `model` and `prompt` under it, which "doesn't
  correspond to anything". An agent's note: the example assumed that Silo itself calls a model for
  a plugin that is only data. No entry says so, nor whose sign-in it would use.
- The owner floats "extension" as the better word, as in an IDE, where an extension declares a
  front end and a back end.
- Read on Theia's pages on 21 November 2026: a "Theia extension" is "installed at compile time",
  "can access the full API of Theia", and declares its front end in `theiaExtensions`. What is
  "installable at runtime", runs "in a dedicated process" and works "against a defined API" is
  called there a VS Code extension.

## The owner's answers (21 November 2026, later)

- "Extension" is the word, in place of "plugin". An extension is a back end, the program, and a
  front end, which is panels that fit in the app the way the app is designed. Silo can offer
  useful pieces for the front end, such as notifying and showing a panel in different ways. The
  owner likens it to the mods of Claude Code. The rename is done with the rewrite of this part.
- Examples of what an extension may use are generic and exact, such as "read this path on main",
  not "read the conversations of main". Finding the conversations in what it reads is the
  extension's job.
- The owner asks for one design of the whole: how an extension fits in, before single points are
  settled. "Providing an output to a precaution" is not the purpose of an extension, and "on not
  safe: deny" does not say what is denied.
- Read on Claude Code's pages on 21 November 2026, through a summarising fetch: a mod registers
  handlers on events; a panel is an element tree the handler returns and the app draws; "Mods
  aren't sandboxed"; the set of answers a hook may give differs by event.

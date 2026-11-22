# Can a rule deny a tool call by its text

## The choice

An agent's part, 22 November 2026. You set a rule on main that denies `git push --force`. Claude
Code's model asks for `git push --force origin main`.

The options:

- A pattern on the text (recommended). The rule names the shell tool and a command that starts with
  `git push --force`, so this call is denied. It costs a limit that stays on the text:
  `git push -f`, or a script that pushes, gets through, and the rule says so.
- The exact text only, as proposed so far. The rule matches one whole command. It costs this very
  case: `git push --force origin main` is not `git push --force`, so it passes.
- No rule on tool calls. Limits are set only on what leaves the computer. It costs any way to deny a
  command that acts only inside the computer, such as deleting a folder, before it runs. A restore
  can only undo it afterwards.

Recommended: a pattern on the text. It is how the permission rules of agent programs already work,
and the rule says plainly what it misses.

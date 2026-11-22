# Which agent programs run in a computer

## The choice

An agent's part, 22 November 2026. A new agent program comes out, and Silo does not offer it yet.
You install it in main with its vendor's installer.

The options:

- Any program runs, as it is (recommended). It runs in main like on any machine, and Silo records
  what it does from outside. It costs what Silo cannot do for a program it does not know: it cannot
  take its conversation up again in a fork, and a fork may carry its login, which Silo tells you
  before making the fork.
- Silo adapts the programs it supports. Silo starts each through a launcher of its own, or changes
  its settings, to know every run and conversation for sure. It costs breakage at each update of a
  program, and a program Silo does not support gets none of it.

Anthropic has a rule for a product that preinstalls or runs Claude Code: the program is not
modified, and none of its ways of signing in is removed, disabled or restricted. Whether it applies
to Silo is not known yet (`recipe-under-preinstall-rule`). The recommended answer keeps the first
condition for every program either way, and `logins` keeps the second. Silo plans neither change, so
keeping them costs nothing now.

Recommended: any program runs, as it is. People switch agent programs often, and Silo sees what any
of them does from outside.

## Anthropic's rule

Read on 19 November 2026: Anthropic has a rule for a product that preinstalls or runs Claude Code:
the program is not modified, and no sign-in method built into it is removed, disabled or restricted.
The rule dates from August 2026, and Anthropic has published no explanation of it.

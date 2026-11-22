# Asking each time a secret or host is used

## The choice

An agent's part, 22 November 2026. You set a rule on main: ask each time the secret `npm-publish` is
used. An agent runs `npm publish`. The request waits, and you get an ask that shows the package and
its version.

The options:

- Build it (recommended). You say yes to each publish, and nothing else in main prompts you. It
  costs your attention each time, and an ask set on something used often teaches you to click yes
  without reading.
- Leave it out. A secret is given or taken back, with nothing between. It costs you the habit of
  giving the secret before each publish and taking it back by hand after.

Recommended: build it. Kept to a few named things, it is the one look a person wants, and work
inside a computer still never prompts per command.

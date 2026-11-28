# The `silo` command

## The choice

An agent's part, 24 November 2026. In the morning you run `silo`. It says main ran all night, took
96 checkpoints and changed 1,240 files. `silo changes 02:40 03:10` lists them, `silo restore 02:40`
puts main back. Nothing of it needs the app.

The options:

- The command first, the app's places later (recommended). Each feature comes with its command, and
  the app reads the same thing when it arrives. It costs the first build a window: there is none.
- The app first, as `app-as-text` orders it today, since it needs `now-in-the-app`. It costs the
  first build months of app work before anything can be used.

Recommended: the command first. `silo-command-reads-the-app` is taken, and the record's own entry
already prints JSON lines (`record-queried-with-ordinary-tools`).

## Notes

Agent, 24 November 2026. Every command and its output on one night are in draft #354
(`details/cli.md`).

An agent's part, 28 November 2026. On polarzero's direction, `silo` runs on your Mac in MVP while
the Swift Mac app (`mac-app`) drives the same API so you do not have to touch the command line.
Scoped `silo` tools inside a computer come after MVP.

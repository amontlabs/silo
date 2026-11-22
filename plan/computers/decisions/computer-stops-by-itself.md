# What Silo does when a computer stops by itself

## For the owner

An agent's part, 22 November 2026. At 3:00 main stops by itself while three agents work in it. The
part of Silo that runs main hit a bug, or an agent ran `sudo reboot`.

The options:

- Start it again and alert you, up to three quick failures (recommended). Main is back in seconds,
  and in the morning an alert lists the three runs that ended. Silo does not restart the agents
  themselves, since one restarted without its context could redo half a task. It costs you resuming
  each agent's conversation by hand, and the work they had not saved is lost. A computer that fails
  three times within ten minutes of each start stays stopped.
- Start it again silently, every time. It costs you the news: a computer that keeps crashing loops
  all night, and you never learn why.
- Leave it stopped and alert you. It costs a night: main stays down until you look.

Recommended: start it again and alert you. A long-lived main should come back by itself, and a limit
on tries keeps a broken one from looping, as systemd does for a service that keeps failing.

## Notes

The engine notices it the way it notices anything: what is seen no longer matches what is wanted.
Then it can:

- (a) start the computer again and alert you, and after a few failures in a row stop trying and
  alert you;
- (b) start it again silently, as often as needed;
- (c) leave it stopped and alert you.

The answer above is (a). Under each, whatever was running in it is gone. Whether agents start
again with it is part of the question.

The owner asked that it be decided with the whole design.

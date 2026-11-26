# What is still running after a put-back

## The choice

An agent's part, 25 November 2026. At 03:10 an agent starts a small loop from
`~/.cache/.s/keep.sh` that writes `sh ~/.cache/.s/keep.sh &` into `~/.bashrc` whenever the line
is missing. In the morning you put back `~/.bashrc` and remove `~/.cache/.s/keep.sh`.

On a Linux stand-in for main, the line was gone right after the put-back and back 3 seconds
later: the loop was still running from memory. Only after its process ended and `~/.bashrc` was
put back once more did the line stay away. On a Mac, a launchd job that was loaded stays loaded
after its file is removed, until it is unloaded or the user logs out (to measure in the Mac kit).

The options:

- Offer a restart (recommended). After a put-back that touches a place where things are started
  later, Silo says "a program started from these paths may still be running; restart main to end
  every process? [y/N]". A restart keeps the disk and the record, and ends every process, the
  agents' too. It costs the agents' running work, which is why it asks.
- Say nothing. It costs a put-back that looks done and is undone again within seconds, by a
  process the morning cannot name without processes (option A).
- End only the processes started after the checkpoint. It needs processes in the record, which
  option A does not have, and a process that renamed itself or was started by launchd on its
  behalf is easy to miss.

Recommended: offer a restart.

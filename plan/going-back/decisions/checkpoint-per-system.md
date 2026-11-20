# What a checkpoint is made of on each system

The answer is worded from the table of what can be seen per system, which comes from the plan's
notes:

| | Linux computer | macOS computer |
|---|---|---|
| Checkpoint | A snapshot of the disk. Memory later. | A clone of the disk. A memory state can be saved, and is refused after some updates of the host. |

A checkpoint that keeps memory is the later feature `checkpoints-with-memory`.

# What going back to a whole computer at one moment promises

## A test this promise waits on, 25 November 2026

An agent's part. #352 (file 07) keeps this answer: a whole computer at one moment is consistent the
way a disk after a power cut is. What it adds is a test that the agents' conversations survive that:
take a checkpoint while Claude Code is writing its conversation file, restore it, and run
`claude --resume`. If the conversation does not come back, the answer's "returns the disk and the
conversations to the same moment" is not shown to hold, and comes back to polarzero. The test is not
in the Mac test kit of #359 yet; it belongs there, beside its tests of checkpoints.

# Whether Silo offers a rule that names the paths to leave out of checkpoints

## The choice

An agent's part, 22 November 2026. An agent in main rebuilds a Rust project several times a day, and
a full build rewrites its 4 GB `target` folder. Each checkpoint keeps the version of that folder it
saw, so thirty daily checkpoints can hold up to 120 GB of output that a build makes again in
minutes.

The options:

- A rule you set (recommended). You name the folders, or accept the rule Silo proposes, and
  checkpoints taken from then on leave them out. After a restore those folders are empty and a build
  fills them. It costs that rebuild after each restore, and it is offered only if a way to build it
  is found.
- No such rule. Every checkpoint keeps the whole disk. It costs disk on your Mac, which only
  thinning old checkpoints keeps down.

Recommended: a rule you set. Most of what a checkpoint would keep is output a build brings back, and
Time Machine and backup tools leave such folders out the same way, by a list the user controls.

## Notes

An opinion cannot decide what a checkpoint leaves out (`opinion-never-chooses-what-is-kept`).
A rule that names the paths as its own data could, the way a rule names package registries.

Put forward, and not answered:

- Silo can propose that rule from its opinions, and it changes nothing until you accept it.
- Such a checkpoint says what it left out (`partial-checkpoint-says-so`).

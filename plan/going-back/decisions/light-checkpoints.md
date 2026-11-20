# How checkpoints are kept light

One large build rewrites gigabytes, and each checkpoint keeps the old ones. What a rebuild brings
back would need no long history. But the opinion of `what-would-bring-it-back` cannot decide what
is left out: a wrong opinion would lose history that cannot be computed again
(`opinion-never-chooses-what-is-kept`).

This is agreed as a direction. Any number for the weight of checkpoints is a rough estimate, to
find out by measuring.

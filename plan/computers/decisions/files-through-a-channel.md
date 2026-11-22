# How files go in or out of a computer

## For the owner

An agent's part, 22 November 2026. You want to open the images an agent made in main in a Mac app,
and to give the agent a 2 GB data file from your Downloads folder.

The options:

- Only through a channel you grant (recommended). You copy the images out and the data file in when
  you ask, or open main's folders through the way in from your Mac, a terminal or your editor over
  SSH. Nothing of main shows on your Mac, and nothing of your Mac in main, until you act. It costs a
  step each time: a Mac app that cannot open files over SSH needs a copy.
- A folder of main always visible on your Mac, as OrbStack shows its machines' files. It costs a
  path outside every rule: what an agent writes there lands where your Mac's apps open, index and
  preview files.

Recommended: only through a channel you grant. A hostile agent's files should reach your Mac only
when you ask for them, and every crossing is then one you can see and close.

## Notes

Channels, and how one is granted, are in `boundary`.

## Two features on files (21 November 2026)

The owner asked for a file channel between computers, governed by rules, and for import from the
user's machine only when the user asks. They are the features `files-between-computers` and
`files-imported-from-your-mac`, which hold the owner's words.

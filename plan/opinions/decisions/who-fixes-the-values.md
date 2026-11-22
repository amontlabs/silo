# Who fixes the values

## The choice

An agent's part, 22 November 2026. An extension named Leftovers wants to tell you that
/work/app/node_modules is "safe to delete".

The options:

- Silo fixes each list (recommended). Silo asks how a path would come back, with three answers: by a
  pull, by a rebuild, only here. Leftovers answers "by a rebuild", the app shows it in Disk beside
  Silo's own opinion, and a precaution you set can read it. It costs the people who write extensions
  a say: one that needs a new kind of answer waits until Silo offers it. Until then, an extension
  such as Injection check, which would say of each model call of main whether it looks like a prompt
  injection (yes, no or unsure), can only alert you, and no precaution can act on what it found.
- Each extension brings its own list. Leftovers answers "safe to delete". You see each extension's
  list when you import it, and a precaution can act on any value in it. It costs what Silo can do
  with an answer it does not understand: it cannot show it in Disk, or compare it with your answers
  or its own opinion. Two extensions also say the same thing in different words, and your
  precautions name words each one invented.

Recommended: Silo fixes each list. GitHub does the same with checks: an app picks its result from
GitHub's list, and only the summary is its own text.

## Notes

The same choice was put in `extension-gives-an-opinion`, which recommended that an extension may
bring its own list in its manifest. It is asked here only, and that entry follows this answer.

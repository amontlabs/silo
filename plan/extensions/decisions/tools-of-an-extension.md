# Which tools an extension may use

## For the owner

An agent's part, 22 November 2026. An extension called Leftovers reads the file lines of main and
says how /work/app/node_modules would come back: by a rebuild. Disk shows that opinion on the folder,
and a precaution you set could act on it.

The decision you took, `extension-tools`, gives three kinds of tool, like the scopes of a GitHub
token: reads, shows and protects. Giving that opinion is none of them. It reads nothing. It is not an
alert, a question or a panel. It does not deny, hold or pause, though a precaution can act on it. So
under the decision as taken, no extension can give an opinion, and an extension cannot be a
classifier, as `what-makes-a-classifier` and `extension-gives-an-opinion` propose.

The options:

- A fourth kind, "gives an opinion" (recommended). When you import Leftovers, its list says it gives
  an opinion on main's file lines. At worst its opinion is wrong: it is shown as Leftovers' opinion,
  and it acts only through a precaution you set, which costs you time. It costs one more kind to read
  when you import an extension.
- Count it as "shows". The opinion appears in the app, like an alert. It costs the kind its meaning:
  what an extension shows wastes your attention at worst, while an opinion can trigger a precaution
  that pauses a computer.
- Keep three kinds. Only Silo's own classifiers give opinions. It costs every judgement Silo did not
  think of: an extension could only alert you, and no precaution could act on what it found.

Recommended: a fourth kind. You asked for a list like the scopes of a GitHub token, and an opinion
has a target and a worst case of its own. Your words of 21 November also say that an answer belongs
to a tool.

## Notes

Everything else of `extension-tools` stays word for word, and its notes keep your words. Its table,
with the fourth row:

| A tool | Target | What the extension may do | Worst case |
|---|---|---|---|
| reads | a path on a computer; the record of a computer | read | what it reads can leave by the network rules of the extension's own computer |
| shows | the app | alert, question, panel | it wastes the user's attention |
| protects | the requests of a computer; a computer | deny, hold; pause | the user loses time |
| gives an opinion | lines of the record it read | give an opinion | a wrong opinion, shown as the extension's, which acts only through a precaution the user set: the user loses time |

`extension-gives-an-opinion` says how such an opinion is kept, and needs this entry.

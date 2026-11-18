## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes becomes a line of `plan/decisions.json`.

- When a user starts, does main (a) copy their agent settings, dotfiles and repositories from the Mac, or (b) start from a guided recipe only?
  - Recommendation: talk it through. (a) makes the first day fast; (b) keeps main rebuildable and keeps files from the Mac out of it.
- A user can try Silo on one project or one task and leave again without moving anything else.
  - Recommendation: talk it through. agent-lives-inside asks the user to move their agent into main, which a one-task trial skips.
- Silo on the user's own device needs no Silo account, sign-up or API key.
  - Recommendation: yes. Being asked for an account is a stated reason users left tools like this one.
- Silo works with no setting touched, and every setting has a default that works.
  - Recommendation: yes. make-main promises a working main from one command, which needs this.
- A user leaving Silo can take each computer as a standard disk image and each recipe and rule as text.
  - Recommendation: yes. Without it there is lock-in, and export already writes a computer to one file.
- Silo works with git and GitHub as they are and asks a team to adopt nothing beyond them.
  - Recommendation: yes. A developer in a company can then start alone, as first-user expects.

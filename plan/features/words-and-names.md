## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes becomes a line of `plan/decisions.json`.

- Silo adds a new word or concept, in the app or in the calls, only when none of its existing words fits.
  - Recommendation: yes. It extends git-words' one word per idea to the whole model.
- A name or a command says what the user gets, with no inside joke and no name of the technology underneath, such as a hypervisor.
  - Recommendation: yes. Technology names change with the engine and mean nothing to a user.
- Before a command or call name is fixed, the main models are asked what it means, and a clash with Git's meaning changes it.
  - Recommendation: yes. Agents are the main callers and read names with Git's meanings; asking takes minutes.
- Each new call and screen is tried on a person or a fresh model that has not read the docs, and must make sense to them.
  - Recommendation: yes. A fresh model makes the check cheap enough to run on every change.
- For forking, reading a diff, merging and granting access, the first place a new user looks in the app is the right one.
  - Recommendation: yes. These four are the loop and its safety, where a new user must not get lost.
- 'Durable' is said only of engine state and 'persistent' only of a computer's disk; a running process is neither unless a checkpoint keeps its memory.
  - Recommendation: yes. It stops a claim that a program survives a restart when only its disk does.
- Silo's words are chosen so that they translate cleanly into other languages.
  - Recommendation: no. The words are Git's, which developers use in English everywhere, and no translation of the app is planned.

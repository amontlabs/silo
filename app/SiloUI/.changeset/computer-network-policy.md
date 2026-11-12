---
"silo-ui": minor
---

Each computer now has a network policy: open (any public address, as before), an allowlist (only the hosts you list, plus GitHub's hosts when the computer has GitHub access and the hosts of the secrets assigned to it), or closed (nothing out). Choose it with `silo network policy <computer> --mode allowlist --allow example.com --preset npm`. The policy is saved with the computer: a checkpoint restore keeps it, a fork starts with it, an export leaves it out and an import starts closed. Computers you already have stay open. A change applies the next time the computer starts; a running computer keeps its rules until then, and the command says so. An agent inside a computer can read its policy and narrow it, and asks you before it widens one. An allowlist does not stop a computer from sending data to a host on its list.

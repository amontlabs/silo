## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes becomes a line of `plan/decisions.json`.

- Everything Silo guarantees is done by the engine outside the computer, and none of it rests on a prompt or on the agent program's own checks.
  - Recommendation: yes. It follows from security-framing: a hostile agent, or one with its checks off, ignores a prompt.
- Access is what each computer holds, shown in one place, and Silo has no roles and no permissions per person.
  - Recommendation: talk it through. It keeps access readable, but company-secrets already hands out secrets per person.
- Silo's access rules limit what a computer can read and where it can send data, because untrusted content always gets in.
  - Recommendation: yes. It explains why secrets and network rules are the parts of access that matter.
- A fresh install with nothing configured is the safe setup, and every default is chosen as if nobody will ever change it.
  - Recommendation: yes. It holds with a list of hosts as the network default; a closed one would block installing the agent.
- For an agent inside a computer, the safe way to do something is also the easiest, and Silo makes it the default there.
  - Recommendation: yes. security-framing asks this for the user, and an agent takes the easiest path too.
- Each feature that crosses a computer's boundary, from ports to the clipboard, GitHub access and merge, states what it lets through and in which direction.
  - Recommendation: yes. Breaches come through the doors far more often than through the wall.
- Anything that decides, such as an approval or a merge, takes a strict typed input and refuses what it cannot read, with no best-effort parsing.
  - Recommendation: yes. A loose parse is how a hostile agent turns a deny into an allow.
- Names of computers, secrets, repositories and rules are protected like their contents, in the app, in the logs and between devices.
  - Recommendation: talk it through. A secret's name can say what it opens, but hiding names makes logs and the app harder to read.
- Each feature of the loop says in its notes which of its steps the engine does and which it leaves to the agent.
  - Recommendation: yes. It shows a builder which steps a guarantee depends on.
- Each promise Silo makes is one sentence that serves as the site's claim, a property the simulator checks and a test a hostile script attacks.
  - Recommendation: talk it through. Tying the site to checked properties is right, but one wording for all three may blur the property.

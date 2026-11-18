## Proposed

- Everything Silo guarantees is done by the engine, outside the computer. None of it rests on a prompt or on the agent program's own checks.
- Access rules limit what a computer can read and where it can send data, because untrusted content always gets in.
- A fresh install with nothing configured is the safe setup. Every default is chosen as if nobody will ever change it.
- Inside a computer, the safe way to do something is also the easiest one for an agent, and it is the default.
- Each feature that crosses a computer's boundary (ports, the clipboard, GitHub access, merge) states what it lets through and in which direction.
- Anything that decides, such as an approval or a merge, takes a strict typed input and refuses what it cannot read. Nothing is parsed on a best effort.
- Each feature of the loop says in its notes which of its steps the engine does and which it leaves to the agent.

## Open

- Is access only what each computer holds, shown in one place, with no roles and no permissions per person? Secrets handed out by a company go per person.
- Are the names of computers, secrets, repositories and rules protected like their contents, in the app, in the logs and between devices? A secret's name can say what it opens, but hidden names make the logs and the app harder to read.
- Is each promise one sentence that serves three times: as the site's claim, as a property the simulator checks, and as a test a hostile script attacks? One wording for all three may blur the property.

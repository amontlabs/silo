# Silo's own extensions

An agent's doubt, from drafting this feature on 21 November 2026.

It may be a condition on what ships with Silo more than a feature of its own. Which extensions
ship is not said. `what-makes-a-classifier`, proposed, would make the classifiers that ship with
Silo (`built-in-classifiers`) extensions; the features that give their opinions come earlier in
the order and need none of the features of this part.

## The owner's words on a list of smaller features (21 November 2026)

The owner wants them "in our plan on the roadmap so we decide later when we get to specifics", and
said of the list:

> most of these really being stuff that could be extensions on silo that either we ship builtin or
> people might want to contribute.

The owner said "most" and did not say which. On the secret asked mid-task, the owner added (21
November 2026):

> Stuff such as asking a secret for instance could just be a silo tool shouldn't be?

An agent's reading, which the owner asked for: the list holds four kinds of thing, and only the
last are extensions.

| Kind | Entries | Why |
|---|---|---|
| Part of Silo: it grants, carries data across the boundary or ends something | `stopping-a-run`, `macos-screen-from-a-linux-computer`, `secret-asked-mid-task`, `agent-programs-updated-everywhere` | No tool of an extension grants or destroys (`what-an-extension-never-does`), and an extension is someone else's code. `agent-programs-updated-everywhere` runs a command in main, which no task does either, so it is an action of Silo like stopping a computer. |
| A `silo` tool inside the computer: the agent's side of the above | `secret-asked-mid-task`; also the sending side of `files-between-computers` and the opening side of `sign-in-from-a-computer` | The agent asks; a rule you set or you answer. The agent never grants itself anything. |
| A rule on a fact, shipped with Silo | `budgets-on-rates`, `pause-on-a-schedule`, `plan-limit-reached` | The trigger is a fact read at the boundary or the clock (`less-sure-less-power`). Taking a checkpoint is not a tool an extension has (`extension-tools`). |
| An extension, shipped or contributed | `memory-and-cpu-per-run`, `stuck-run-alert`, `runs-writing-the-same-checkout`, `spend-split-and-forecast`, `merges-and-deploys-held` | It reads the record and shows, alerts or holds, which is what `extension-tools` allows. `merges-and-deploys-held` is a good one to ship: it protects without granting. |

These extensions are only as good as the facts the record holds. What each one needs from the
record is in the notes of `processes-in-the-record`, `model-calls-in-the-record` and
`what-each-run-wrote`.

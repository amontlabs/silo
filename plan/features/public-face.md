## Proposed

- Two kinds of docs
  - Rule: The docs are a complete reference for agents and a few short pages for people.
  - Rule: The pages for people are written by a person, and designed together with one.
  - Property: The whole agent reference fits under a stated token count, with one example per command and no navigation or text a model would guess right.
  - Rule: Each example for agents is in exactly the form an agent should write, never a test's internal form, and shows the command that runs it.
  - Every page of the site and the docs has a Markdown twin, and the site has an llms.txt, both generated from the source of the pages people read.
  - Docs: Silo's tests, agent files and reference code for integrations are published for agents to search and copy, outside the navigation people use.
  - The agent reference keeps tutorials, how-to guides, reference and explanation apart, as Diataxis does.
- Docs that are tested
  - Rule: Each real question from a user and each place a newcomer's agent got stuck becomes a task in the agent test set, and its answer moves higher in the docs.
  - Test: Each release checks that every Markdown twin and the llms.txt exist and match the current pages, separately from the check of the pages people see.
  - Test: The build and each release check every link of the site and docs, download links included, and fail on anything published that relies on a symlink.
  - Test: Ask an agent 'should we use Silo?' and check that the install size, the package and the docs it bases its answer on are current and correct.
  - Test: A check over the public pages flags every formulation that WRITING.md lists.
  - Test: After each rewrite of the home page, someone who never heard of Silo reads it for one minute and says what Silo does, and the answer is recorded.
- Where a reader starts
  - The docs open on one small run in a fork (a package installed, the diff showing it, then a merge or a drop), then examples, then concepts and the reference.
  - The docs' first page answers whether a computer persists, how you get into it, how much memory it takes and whether it can build macOS apps.
  - Rule: No sign-up, terms wall or login stands in front of the docs or the posts.
  - Rule: Each Silo word is defined where the docs first use it, starting from something the reader knows, with no knowledge of virtual machines assumed.
  - Docs: One page maps the words readers arrive with (Docker, Git, VM snapshot, devcontainer, worktree) to Silo's (computer, fork, checkpoint, recipe) and shows how the parts relate.
  - Docs: Each audience, such as Claude Code on a Mac or a Linux server, gets one permanent path of one or two pages with one small use case.
  - Docs: An examples folder holds small combinations of what the demo shows: a plain loop; plan, implement, review; working through a queue of tickets.
- What the pages state
  - The docs list Silo's limits: the numeric ones (computers, forks, sizes, name lengths), the edge cases, and the cases where Silo is the wrong tool.
  - Docs: One page gives the measured numbers: download size, engine memory, start and fork times, memory and CPU per computer, suspend and resume times, disk per fork.
  - A benchmark harness such as hyperfine measures those numbers, its reports are committed, and partial results are marked as partial.
  - The docs name the virtual machine layer Silo runs on and what Silo adds on top of it.
  - Rule: The docs call something durable only with the test it passes, such as the same request sent twice, never because it retries or writes to a database.
  - Rule: Where two tools or formats exist, the docs show both side by side in tabs, with no site-wide switch.
  - Rule: The docs and examples never assume one vendor's agent program.
  - Rule: The docs never present local models as the way to run agent programs in Silo.
  - Docs: If the site compares Silo with other tools, it covers containers, cloud sandboxes, worktrees (Conductor's and Claude Code's), a second laptop and the agent on your own laptop.
  - Docs: A comparison with worktrees says a worktree isolates a folder, and Silo also isolates packages, databases, ports and secrets.
  - Docs: If the site says how Silo contacts users, it says Silo never asks anyone to run a command during a call.
- Keeping it true
  - Rule: The public docs describe the latest release, and anything only on the main branch is marked unreleased.
  - Rule: A feature is documented only once it has been used for real.
  - Rule: Before an agent-written page is published, every quote, price and claim about another product in it is checked against a source.
  - Docs: The README and the site always say what is unfinished and which parts are ready for real work, as the heavy-development notice does now.
  - Rule: The README has no relative dates and no date that goes stale.
  - Every page of the docs links to the changelog.
  - A list is kept of every public page, deployment and address Silo has used, and each old one is removed or redirected.
  - Test: Opening github.com/0xpolarzero/silo lands on the current repository.
  - Docs: Any public mirror that is not the project's home says so and points to where issues go.
- How it is built
  - The docs are built with Vocs and served with the site, since a hosted docs vendor adds a cost and a dependency.
  - The docs are static pages from our own build, with prefetch on hover and transitions between pages, and nothing billed per server.
  - A new page starts from a strong visual reference and a headless component library, and an agent writes the styling.
  - Where a page needs them, use what has worked with agents: TanStack Virtual and Pacer, and shadcn components.
  - Rule: Lint rules for the site's code forbid useEffect and unsafe `any`, and Effect's own lint rules apply through the type checker if the code uses Effect.
  - If visitors can try real computers, they come from a fixed set, each used by one visitor at a time.
  - Risk: A docs chat, if built, costs about 32k input tokens a question and can be abused, so it limits who may call it and stays hidden until it answers correctly.

## Open

- Is the feature's demo the home page's drawn flow as it is, recordings of the real app, the app's screens on sample data that visitors click through, or real demo computers?
  - Suggested: The flow plus recordings of the real app, such as one click undoing a wrecked computer, since the other two need a second app or paid computers.
- Do the README and the pages for people teach what to say to the agent in main, or the `silo` commands, such as one that puts a repository in a computer?
  - Suggested: What to say to the agent: it lives in main and runs the commands (agent-lives-inside), which stay in the agent reference.
- Should the first screen and the README's opening also answer what visitors ask: what it runs on, whether it is open source, whether their agent and subscription still work, how much memory it takes?
  - Suggested: No: the cards already name Linux, macOS and a Linux server, and the docs' first page can answer the rest.
- Which does the site add: a page per feature with a 90-second video, a comparison with containers, cloud sandboxes and worktrees, the engine's live check results, a docs chat Silo pays for?
  - Suggested: The comparison only, since visitors cannot tell the options apart; the cards, the heavy-development notice and their own agent cover the rest.
- Which wording for what the engine guarantees: 'formal proofs', as first said, or never 'proven', as an earlier writing rule asks?
  - Suggested: 'Proven' only where a formal proof exists, and for the rest 'checked by the simulator' with what it checked.
- Which written promises does the site make: whether you ever need an account and what is kept about you, how Silo will contact you, a status page that counts every incident?
  - Suggested: The account and data statement now; contact and incidents once Silo has users to write to or a service to keep running.
- To find out: Run the agent test set with each supported agent program to learn whether one set of examples serves them all or each needs its own.

## From the old code

The old code had it built and how much can be reused is not known.

Lessons:
- The site and README still describe an older product (no allowlist, protocol 7, desktop-first), so copy drifts from the product.

Where it lived, at the tag legacy-2026-11-16:
- `website/index.html`
- `website/src/demo/read-only-demo.tsx`
- `docs/SiloUI-RELEASE-FILM.md`

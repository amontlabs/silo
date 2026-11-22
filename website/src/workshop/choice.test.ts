// The parser of "The choice" on three entries' notes as the plan writes them, and what it leaves as Markdown: a section
// with another shape shows whole, and no piece of the notes is lost.
import { describe, expect, it } from 'vitest';
import { holdsOnlyChoice, parseChoice, splitNotes } from './choice';
import type { ChoiceTable } from './plan';

const WORD_AGENT = `# The word "agent"

## The choice

An agent's part, 22 November 2026. You start Claude Code in main and it starts four subagents. You
close it and open it again an hour later. How many agents was that: one, two, five?

The options:

- The everyday word, never counted (recommended). "An agent program at work in a computer, with its
  model." It is read as loosely as people say it. Where it matters who did something, Silo says the
  run or the conversation. It costs the user two more words to learn.
- An agent is a run. One launch of an agent program is one agent. It is read as "the thing I
  started". It costs the subagents, which are then not agents, and reopening Claude Code makes a new
  agent.
- An agent is a conversation. Each sequence of turns with one model is an agent. It is read as "each
  thing that thinks". It costs one Claude Code showing as five agents.

Recommended: the everyday word, never counted. People will keep saying "agent" for all three, so the
word that counts must be another one, and "run" and "conversation" are already in force.

## Notes

The design record uses "agent" throughout and never defines it: section 2 has rows for "agent
program" and "run", none for "agent".
`;

const SPEND = `# Spend

## The choice

An agent's part, 22 November 2026. Today in main one conversation cost 12.30 dollars and two others
4.10 and 0.80, so main spent 17.20 in all.

The options:

- Build it (recommended). You see what each conversation and each computer spent. It costs a price
  list for each model, which Silo keeps up to date, and a model it has no price for shows as
  unknown.
- Leave it out. You see spend only on the provider's own page, for your whole account. It costs
  spend per computer and per conversation, and the budgets that are built on it.

Recommended: build it. Budgets need it, and it tells you which work your money goes to.

## Notes

An agent's doubt, from drafting this feature on 21 November 2026.

Spend per computer comes from \`network-place\`, a decision of What you see, not from a decision of this part. No decision says where prices come from.
`;

const ENGINE = `# The engine and the programs that follow it

## The choice

An agent's part, 22 November 2026. The app is open. In a terminal you type the command that forks
main.

The options:

- Build it (recommended). The engine runs as a program of its own on your Mac, and the app and the
  command line both follow it. The fork shows in the app as it is made, the two always show the same
  state, and a call sent twice makes one fork. It costs the largest first step of the plan: sixteen
  decisions come before it, the simulator is built first, and by itself it shows almost nothing.
- Leave it out. The app would do the work itself, with no engine behind it. It costs every other
  feature of the app, since nine entries already wait on this one, and it goes against the engine
  being its own program, which is decided.

Recommended: build it. Nothing else in the app can be built before it.

## Notes

An agent's doubt, from drafting this feature on 21 November 2026.

It is large: it needs 16 decisions. Most of the engine's decisions say how the engine is made, so its features are cut at what can be watched working: the app and the command line showing the same state.
`;

const read = (notes: string) => parseChoice(splitNotes(notes)!.choice);
const table = (notes: string) => read(notes) as ChoiceTable;

describe('the choice', () => {
  it('reads a decision: the case without who wrote it, each option with what it means and what it costs, and why', () => {
    expect(read(WORD_AGENT)).toEqual({
      case: 'You start Claude Code in main and it starts four subagents. You close it and open it again an hour later. How many agents was that: one, two, five?',
      options: [
        { name: 'The everyday word, never counted', what: '"An agent program at work in a computer, with its model." It is read as loosely as people say it. Where it matters who did something, Silo says the run or the conversation.', cost: 'It costs the user two more words to learn.', recommended: true },
        { name: 'An agent is a run', what: 'One launch of an agent program is one agent. It is read as "the thing I started".', cost: 'It costs the subagents, which are then not agents, and reopening Claude Code makes a new agent.' },
        { name: 'An agent is a conversation', what: 'Each sequence of turns with one model is an agent. It is read as "each thing that thinks".', cost: 'It costs one Claude Code showing as five agents.' },
      ],
      why: 'People will keep saying "agent" for all three, so the word that counts must be another one, and "run" and "conversation" are already in force.',
    });
  });

  it('reads a feature: build it, or leave it out', () => {
    expect(read(SPEND)).toEqual({
      case: 'Today in main one conversation cost 12.30 dollars and two others 4.10 and 0.80, so main spent 17.20 in all.',
      options: [
        { name: 'Build it', what: 'You see what each conversation and each computer spent.', cost: 'It costs a price list for each model, which Silo keeps up to date, and a model it has no price for shows as unknown.', recommended: true },
        { name: 'Leave it out', what: "You see spend only on the provider's own page, for your whole account.", cost: 'It costs spend per computer and per conversation, and the budgets that are built on it.' },
      ],
      why: 'Budgets need it, and it tells you which work your money goes to.',
    });
  });

  it('reads the first feature', () => {
    const choice = table(ENGINE);
    expect(choice.case).toBe('The app is open. In a terminal you type the command that forks main.');
    expect(choice.options.map((option) => [option.name, option.recommended === true])).toEqual([['Build it', true], ['Leave it out', false]]);
    expect(choice.options[0].cost).toBe('It costs the largest first step of the plan: sixteen decisions come before it, the simulator is built first, and by itself it shows almost nothing.');
    expect(choice.why).toBe('Nothing else in the app can be built before it.');
  });

  it('gives no cost to an option with no sentence that starts "It costs"', () => {
    const { options } = table(SPEND.replace('It costs\n  spend per computer', 'So it also costs\n  spend per computer'));
    expect(options[1]).toEqual({ name: 'Leave it out', what: "You see spend only on the provider's own page, for your whole account. So it also costs spend per computer and per conversation, and the budgets that are built on it." });
  });

  it('keeps what follows the cost in its column, in its order, and an option that says only its cost', () => {
    const { options } = table(`${SPEND.slice(0, SPEND.indexOf('- Build it'))}- Build it (recommended). It costs a list. The list needs care.
- Leave it out. It costs spend.

Recommended: build it. Budgets need it.
`);
    expect(options).toEqual([
      { name: 'Build it', what: '', cost: 'It costs a list. The list needs care.', recommended: true },
      { name: 'Leave it out', what: '', cost: 'It costs spend.' },
    ]);
  });

  it('keeps the words a recommendation adds to the option it names, at the start of the reason', () => {
    expect(table(SPEND.replace('Recommended: build it.', 'Recommended: build it, in its later stage.')).why).toBe('Build it, in its later stage. Budgets need it, and it tells you which work your money goes to.');
    expect(table(WORD_AGENT.replace('Recommended: the everyday word, never counted.', 'Recommended: the everyday word.')).why).toMatch(/^People will keep/);
  });

  const broken: [string, (notes: string) => string][] = [
    ['a second case', (notes) => notes.replace('## Notes', 'A second case: a model has no price.\n\n## Notes')],
    ['no list', (notes) => notes.replace(/\n- [\s\S]*?\n\nRecommended/, '\nBuild it or leave it out.\n\nRecommended')],
    ['no "Recommended:"', (notes) => notes.replace('Recommended: build it.', 'Build it.')],
    ['no reason', (notes) => notes.replace(/Recommended: build it\.[^#]*/, 'Recommended: build it.\n\n')],
    ['no first sentence that says whose part it is', (notes) => notes.replace("An agent's part, 22 November 2026. ", '')],
    ['a mark inside a name', (notes) => notes.replace('Build it (recommended).', 'Build it (recommended), as an extension.')],
    ['no option marked', (notes) => notes.replace(' (recommended)', '')],
    ['two options marked', (notes) => notes.replace('Leave it out.', 'Leave it out (recommended).')],
    ['a recommendation of the option not marked', (notes) => notes.replace('Recommended: build it.', 'Recommended: leave it out.')],
    ['a list inside an option', (notes) => notes.replace('- Leave it out.', '  - per model\n- Leave it out.')],
  ];
  it.each(broken)('leaves a section with %s whole, as Markdown', (_, change) => {
    const notes = change(SPEND);
    expect(notes).not.toBe(SPEND);
    const { choice } = splitNotes(notes)!;
    expect(parseChoice(choice)).toEqual({ text: choice.replace('## The choice', '').trim() });
  });
});

describe('the notes around the choice', () => {
  it('are the notes again when put back together, and end the section at the next section', () => {
    for (const notes of [WORD_AGENT, SPEND, ENGINE, SPEND.replace(/\n/g, '\r\n')]) {
      const { before, choice, after } = splitNotes(notes)!;
      expect(before + choice + after).toBe(notes);
      expect(after).toMatch(/^## Notes/);
    }
  });

  it('say whether anything but headings stands beside it', () => {
    expect(holdsOnlyChoice(splitNotes(SPEND)!)).toBe(false);
    expect(holdsOnlyChoice(splitNotes(SPEND.slice(0, SPEND.indexOf('## Notes')))!)).toBe(true);
  });

  it('take no heading inside a code block for the section, and none for an empty one', () => {
    expect(splitNotes('```\n## The choice\nText.\n```\n')).toBeNull();
    expect(splitNotes('# A name\n\n## The choice\n\n## Notes\n\nText.\n')).toBeNull();
    expect(splitNotes('## The choice\n\n```\n## not a heading\n```\n')!.after).toBe('');
  });
});

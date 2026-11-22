// What an open row shows of the plan's lists and of what it needs, its id and the button that copies it.
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { buildData, DECISIONS, fixtureFacts, FEATURES, LIST_DECISIONS, LIST_FEATURES, LISTS, withLists } from './fixtures';
import { GROW_MS, HOLD_MS } from './Entry';
import type { Facts } from './plan';
import { SiteChanged } from './chunks';
import { itemOf, mockApi, owner, renderApp, rowButton, wait, withheldTimers } from './test-support';

const data = () => buildData(withLists());

async function openFrom(hash: string, name: string, built = data(), loadFacts?: () => Promise<Facts>) {
  mockApi({ me: owner });
  history.replaceState(null, '', `/workshop${hash}`);
  await renderApp(built, undefined, loadFacts);
  await waitFor(() => expect(rowButton(name)).toHaveAttribute('aria-expanded', 'true'));
  return itemOf(name);
}

/** Where each text is on the open row: the positions are all found and rise in the order given. */
const inOrder = (item: HTMLElement, labels: string[]) => {
  const text = item.textContent ?? '';
  const at = labels.map((label) => text.indexOf(label));
  expect(at.every((index) => index >= 0), `${labels.join(' | ')} in ${text}`).toBe(true);
  expect(at).toEqual([...at].sort((a, b) => a - b));
};

const SNAPSHOT = 'What is a saved moment of a computer called?';
const lines = (item: HTMLElement, label: string) => {
  const part = within(item).getByText(label).closest('.ws-fact') as HTMLElement;
  return [...part.querySelectorAll('li')].map((li) => li.textContent);
};

afterEach(() => vi.unstubAllGlobals());

describe('the recommendation', () => {
  it('says "Recommended: take" and why on an entry with no verdict that has a suggestion', async () => {
    const item = await openFrom('#part-b/create', 'Create and delete a computer');
    const part = (await within(item).findByText('Recommended: take')).closest('.ws-fact') as HTMLElement;
    expect(part).toHaveTextContent('Key records by id.');
  });

  it('says "Recommended: drop" for a recommendation to drop', async () => {
    const item = await openFrom('#part-b/retire-scrap', 'Does Silo keep the word scrap?');
    expect(await within(item).findByText('Recommended: drop')).toBeInTheDocument();
    expect(item).toHaveTextContent('Nobody says it.');
  });

  it('says nothing on an entry that has a verdict, whatever its file suggests', async () => {
    const item = await openFrom('#part-a/core', 'The engine runs on its own');
    expect(within(item).queryByText(/^Recommended/)).not.toBeInTheDocument();
    expect(item).not.toHaveTextContent('The core piece.');
  });
});

describe('the choice', () => {
  const CHOICE = `# A name

## The choice

An agent's part, 22 November 2026. You press Create twice.

The options:

- One computer (recommended). The second press finds the first. It costs a key.
- Two computers. Each press makes one.

Recommended: one computer. Nobody wants the second.
`;
  const BESIDE = '\n## Notes\n\nFrom the old code.\n';
  const withChoice = (notes: Record<string, string>) => buildData({ ...withLists(), notes });

  it('shows in place on an entry with no verdict: the case, the options with their costs and why, before what a yes would add', async () => {
    const item = await openFrom('#part-b/name-a-snapshot', SNAPSHOT, withChoice({ 'name-a-snapshot': CHOICE + BESIDE }));
    await within(item).findByText('The case');
    inOrder(item, ['The case', 'You press Create twice.', 'The options', 'The cost', 'One computer', 'recommended', 'It costs a key.', 'Two computers', 'Why this one', 'Nobody wants the second.', 'A yes would add', 'Not decided.']);
    expect(item).not.toHaveTextContent("An agent's part");
    expect(within(item).getByText('Each press makes one.')).toHaveClass('ws-span');
  });

  it('gives a feature its reason once: under "Why this one", the recommendation staying a line', async () => {
    const item = await openFrom('#part-b/create', 'Create and delete a computer', withChoice({ create: CHOICE }));
    await within(item).findByText('Why this one');
    inOrder(item, ['Make a Linux computer.', 'Recommended: take', 'The case', 'Why this one']);
    expect(item).not.toHaveTextContent('Key records by id.');
  });

  it('is left out of Notes while it shows in place', async () => {
    const item = await openFrom('#part-b/create', 'Create and delete a computer', withChoice({ create: CHOICE + BESIDE }));
    await within(item).findByText('The case');
    await userEvent.click(within(item).getByRole('button', { name: 'Notes' }));
    await waitFor(() => expect(item).toHaveTextContent('From the old code.'));
    expect(within(item).getAllByText('You press Create twice.')).toHaveLength(1);
  });

  it('leaves no Notes when the notes hold nothing else', async () => {
    const item = await openFrom('#part-b/create', 'Create and delete a computer', withChoice({ create: CHOICE }));
    await within(item).findByText('The case');
    expect(within(item).queryByRole('button', { name: 'Notes' })).not.toBeInTheDocument();
  });

  it('is read under Notes once the entry has a verdict', async () => {
    const item = await openFrom('#part-a/core', 'The engine runs on its own', withChoice({ core: CHOICE }));
    expect(within(item).queryByText('The case')).not.toBeInTheDocument();
    await userEvent.click(within(item).getByRole('button', { name: 'Notes' }));
    await waitFor(() => expect(item).toHaveTextContent('You press Create twice.'));
  });

  it('is read under Notes when it could not load in place', async () => {
    const item = await openFrom('#part-b/create', 'Create and delete a computer', withChoice({ create: CHOICE }), () => Promise.reject(new Error('offline')));
    await within(item).findByText(/Details could not load/);
    await userEvent.click(within(item).getByRole('button', { name: 'Notes' }));
    await waitFor(() => expect(item).toHaveTextContent('You press Create twice.'));
  });

  it('shows whole as Markdown when it has another shape, and the reason of the recommendation stays', async () => {
    const item = await openFrom('#part-b/create', 'Create and delete a computer', withChoice({ create: `${CHOICE}\nA second case: you press it three times.\n` }));
    await within(item).findByText('A second case: you press it three times.');
    inOrder(item, ['Key records by id.', "An agent's part", 'The options:', 'Two computers.', 'Recommended: one computer.', 'A second case']);
    expect(within(item).queryByText('The case')).not.toBeInTheDocument();
  });

  it('holds the panel briefly for the details, so it opens at its final size', async () => {
    const data = withChoice({ 'name-a-snapshot': CHOICE });
    let arrive!: (value: Facts) => void;
    mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#part-b');
    await renderApp(data, undefined, () => new Promise<Facts>((resolve) => { arrive = resolve; }));
    withheldTimers(HOLD_MS, { leaves: true });
    const name = SNAPSHOT.replace('?', '\\?');
    await userEvent.click(rowButton(name));
    const item = itemOf(name);
    expect(item.querySelector('.ws-more.open')).toBeNull();
    await act(async () => arrive(fixtureFacts(data)));
    await waitFor(() => expect(item.querySelector('.ws-more.open')).not.toBeNull());
    expect(within(item).getByText('The case')).toBeInTheDocument();
    expect(item.querySelector('.ws-ph-notes')).toBeNull();
  });

  it('keeps the placeholder while a panel that could not wait opens, then grows to the choice', async () => {
    const data = withChoice({ 'name-a-snapshot': CHOICE });
    let arrive!: (value: Facts) => void;
    mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#part-b');
    await renderApp(data, undefined, () => new Promise<Facts>((resolve) => { arrive = resolve; }));
    const name = SNAPSHOT.replace('?', '\\?');
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    try {
      fireEvent.click(rowButton(name));
      const item = itemOf(name);
      await act(async () => { await vi.advanceTimersByTimeAsync(HOLD_MS); });
      expect(item.querySelector('.ws-more.open')).not.toBeNull();
      await act(async () => arrive(fixtureFacts(data)));
      expect(within(item).queryByText('The case')).toBeNull();
      expect(item.querySelector('.ws-ph-notes')).not.toBeNull();
      await act(async () => { await vi.advanceTimersByTimeAsync(GROW_MS); });
      expect(within(item).getByText('The case')).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });
});

describe('what a yes would change', () => {
  it('adds what is not in force: the kind as a word, then the word and its meaning', async () => {
    const item = await openFrom('#part-b/name-a-snapshot', SNAPSHOT);
    await within(item).findByText('A yes would add');
    expect(lines(item, 'A yes would add')).toEqual(['wordsnapshot A saved moment of a computer.']);
    const line = within(item).getByText('A yes would add').closest('.ws-fact')!.querySelector('li') as HTMLElement;
    expect(within(line).getByText('word')).toHaveClass('ws-tag');
    expect(within(line).getByText('snapshot')).toBeInTheDocument();
  });

  it('adds a principle as what it says, then the question it asks, and a property as what it says', async () => {
    const fresh = buildData({ ...withLists(), decisions: [...DECISIONS, ...LIST_DECISIONS.map((decision) => (decision.id === 'name-a-snapshot' ? { ...decision, sets: [
      { in: 'principles', id: 'ask-first', says: 'Ask before acting.', asks: 'What does it ask?' },
      { in: 'properties', id: 'asks-end', says: 'An ask ends.', kind: 'liveness', check: ['test'] },
    ] } : decision))] });
    const item = await openFrom('#part-b/name-a-snapshot', SNAPSHOT, fresh);
    await within(item).findByText('A yes would add');
    expect(lines(item, 'A yes would add')).toEqual(['principleAsk before acting. What does it ask?', 'propertyAn ask ends.']);
  });

  it('changes what is in force: the kind and id, then "Now" with the text in force and "After" with the new one', async () => {
    const item = await openFrom('#part-b/name-a-snapshot', SNAPSHOT);
    await within(item).findByText('A yes would change');
    expect(lines(item, 'A yes would change')).toEqual([
      'principleleast-powerNowThe less sure the trigger, the less it may do. What triggers it?AfterThe less sure the trigger, the less it may do, and it asks first. What triggers it, and what does it ask?',
    ]);
    const change = within(item).getByText('A yes would change').closest('.ws-fact') as HTMLElement;
    expect(change.querySelector('code')).toHaveTextContent('least-power');
  });

  it('removes what is in force: each id with its kind and the text in force', async () => {
    const item = await openFrom('#part-b/retire-scrap', 'Does Silo keep the word scrap?');
    await within(item).findByText('A yes would remove');
    expect(lines(item, 'A yes would remove')).toEqual(['wordscrap A computer nobody uses.']);
  });

  it('reads whole from its row for a decision that has no answer and only sets a property', async () => {
    const item = await openFrom('#part-b/bound-a-wait', 'Is a wait at the edge bounded?');
    await within(item).findByText('A yes would add');
    expect(item.querySelector('.ws-what')).toBeNull();
    expect(lines(item, 'A yes would add')).toEqual(['propertyA wait at the edge ends within its limit.']);
    expect(within(item).queryByText(/^Recommended/)).not.toBeInTheDocument();
    expect(within(item).queryByText('A yes would change')).not.toBeInTheDocument();
    expect(within(item).queryByText('A yes would remove')).not.toBeInTheDocument();
  });

  it('shows no such part once the entry has a verdict', async () => {
    const dropped = buildData({ ...withLists(), decisions: [...DECISIONS, ...LIST_DECISIONS.map((decision) => (decision.id === 'name-a-snapshot' ? { ...decision, verdict: 'drop' as const } : decision))] });
    const item = await openFrom('#part-b/name-a-snapshot', SNAPSHOT, dropped);
    await wait(50);
    expect(within(item).queryByText(/^A yes would/)).not.toBeInTheDocument();
    expect(within(item).queryByText(/^Recommended/)).not.toBeInTheDocument();
  });
});

describe('what a taken entry added', () => {
  it('says "Added" with each statement of its adds, its kind and its text from the lists', async () => {
    const item = await openFrom('#product/what-we-call-things', 'What are the words of Silo?');
    await within(item).findByText('Added');
    expect(lines(item, 'Added')).toEqual(['wordtwin A copy of a computer.', 'wordscrap A computer nobody uses.', 'principleThe less sure the trigger, the less it may do. What triggers it?']);
    expect(within(item).queryByText(/^A yes would/)).not.toBeInTheDocument();
  });

  it('does so for a taken feature, with properties', async () => {
    const item = await openFrom('#part-b/leak-guard', 'Stop what leaks');
    await within(item).findByText('Added');
    expect(lines(item, 'Added')).toEqual(['propertyNothing leaves a computer unasked.', 'propertyEvery wait ends.']);
  });

  it('keeps "Replaces" where it was, after "Added"', async () => {
    const changed = buildData({
      ...withLists(),
      features: LIST_FEATURES.length ? [...FEATURES, ...LIST_FEATURES.map((feature) => (feature.id === 'leak-guard' ? { ...feature, adds: ['ends-up'] } : feature))] : FEATURES,
      decisions: [
        ...DECISIONS.map((decision) => (decision.id === 'tamper-proof-logs' ? { ...decision, verdict: 'take' as const, adds: ['never-leaks'] } : decision.id === 'signed-logs' ? { ...decision, replacedBy: 'tamper-proof-logs' } : decision)),
        ...LIST_DECISIONS,
      ],
    });
    const item = await openFrom('#part-d/tamper-proof-logs', 'Can a log be changed after the fact?', changed);
    await within(item).findByText('Added');
    inOrder(item, ['Added', 'Replaces', 'Decided.']);
  });
});

describe('what an entry needs', () => {
  // Relations are drawn by the strip and the lit rows, never written in the opened row.
  it('is not listed in the opened row, even when it needs entries of other parts', async () => {
    const item = await openFrom('#part-b/fork-keeps', 'Keep a fork apart');
    await within(item).findByText('Must keep');
    expect(within(item).queryByText('Needs')).not.toBeInTheDocument();
    expect(within(item).queryByRole('link', { name: 'Which way do changes go?' })).not.toBeInTheDocument();
  });
});

describe('what a feature keeps and where it shows', () => {
  it('says "Must keep" with the text of each property it keeps', async () => {
    const item = await openFrom('#part-b/fork-keeps', 'Keep a fork apart');
    await within(item).findByText('Must keep');
    expect(lines(item, 'Must keep')).toEqual(['Nothing leaves a computer unasked.', 'Every wait ends.']);
  });

  it('says "Where it shows" with its App and its Command', async () => {
    const item = await openFrom('#part-b/fork-keeps', 'Keep a fork apart');
    await within(item).findByText('Where it shows');
    expect(lines(item, 'Where it shows')).toEqual(['AppComputers, then Fork', 'Commandsilo fork <computer>']);
    expect(within(item).getByText('silo fork <computer>').tagName).toBe('CODE');
  });

  it('shows only the value a feature has', async () => {
    const only = buildData({ ...withLists(), features: [...FEATURES, ...LIST_FEATURES.map((feature) => (feature.id === 'fork-keeps' ? { ...feature, surfaces: { cli: 'silo fork' } } : feature))] });
    const item = await openFrom('#part-b/fork-keeps', 'Keep a fork apart', only);
    await within(item).findByText('Where it shows');
    expect(lines(item, 'Where it shows')).toEqual(['Commandsilo fork']);
    expect(within(item).queryByText('App')).not.toBeInTheDocument();
  });

  it('shows neither on a decision', async () => {
    const item = await openFrom('#part-b/retire-scrap', 'Does Silo keep the word scrap?');
    await within(item).findByText('A yes would remove');
    expect(within(item).queryByText('Must keep')).not.toBeInTheDocument();
    expect(within(item).queryByText('Where it shows')).not.toBeInTheDocument();
  });
});

describe('the order of the parts', () => {
  it('goes: the body, the recommendation, what a yes would add, change and remove, then the state', async () => {
    const both = buildData({ ...withLists(), decisions: [...DECISIONS, ...LIST_DECISIONS.map((decision) => (decision.id === 'name-a-snapshot' ? { ...decision, answer: 'A snapshot.', removes: ['scrap'], notes: undefined } : decision))] });
    const item = await openFrom('#part-b/name-a-snapshot', SNAPSHOT, both);
    await within(item).findByText('A yes would remove');
    const labels = ['A snapshot.', 'Recommended: take', 'A yes would add', 'A yes would change', 'A yes would remove', 'Not decided.'];
    inOrder(item, labels);
  });

  it('goes: the body, what it must keep, where it shows, then the state', async () => {
    const item = await openFrom('#part-b/fork-keeps', 'Keep a fork apart');
    await within(item).findByText('Must keep');
    const labels = ['A fork shares nothing', 'Must keep', 'Where it shows', 'Not decided.'];
    inOrder(item, labels);
  });

  it('puts the state before Notes, and Notes before the thread', async () => {
    mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#part-a/core');
    await renderApp();
    const item = itemOf('The engine runs on its own');
    await waitFor(() => expect(item.querySelector('.ws-extras')).not.toBeNull());
    const state = item.querySelector('.ws-state') as HTMLElement;
    const notes = item.querySelector('.ws-extras') as HTMLElement;
    expect(state.compareDocumentPosition(notes) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    await waitFor(() => expect(item.querySelector('.ws-talk')).not.toBeNull());
    expect(notes.compareDocumentPosition(item.querySelector('.ws-talk') as Element) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });
});

describe('the chunk', () => {
  it('is not asked for until a row with something to show is reached or opened', async () => {
    const load = vi.fn(() => Promise.resolve(fixtureFacts(data())));
    mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#part-a');
    await renderApp(data(), undefined, load);
    expect(load).not.toHaveBeenCalled();
    // A row with nothing to show does not ask either.
    await userEvent.click(rowButton('The engine runs on its own'));
    expect(load).not.toHaveBeenCalled();
    await userEvent.hover(rowButton('Every call defined once'));
    expect(load).not.toHaveBeenCalled();
  });

  it('is asked for once, when a pointer reaches a row that has something to show', async () => {
    const load = vi.fn(() => Promise.resolve(fixtureFacts(data())));
    mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#part-b');
    await renderApp(data(), undefined, load);
    await userEvent.hover(rowButton('Create and delete a computer'));
    await userEvent.hover(rowButton(SNAPSHOT));
    expect(load).toHaveBeenCalledTimes(1);
  });

  it('holds the place of the parts with three lines while it loads, then draws them where they belong', async () => {
    let release: (facts: Facts) => void = () => undefined;
    const pending = new Promise<Facts>((resolve) => { release = resolve; });
    const item = await openFrom('#part-b/name-a-snapshot', SNAPSHOT, data(), () => pending);
    const placeholder = item.querySelector('.ws-ph-notes') as HTMLElement;
    expect(placeholder).toHaveAttribute('aria-busy', 'true');
    expect(placeholder.querySelectorAll('.ws-ph[aria-hidden="true"]')).toHaveLength(3);
    expect(within(item).queryByText('A yes would add')).not.toBeInTheDocument();
    await act(async () => release(fixtureFacts(data())));
    expect(await within(item).findByText('A yes would add')).toBeInTheDocument();
    expect(item.querySelector('.ws-ph-notes')).toBeNull();
  });

  it('holds no place for a row with nothing to show', async () => {
    const item = await openFrom('#part-a/core', 'The engine runs on its own', data(), () => new Promise<Facts>(() => undefined));
    expect(item.querySelector('.ws-ph-notes')).toBeNull();
  });

  it('says it could not load, in the place of the parts, with Reload', async () => {
    const item = await openFrom('#part-b/name-a-snapshot', SNAPSHOT, data(), () => Promise.reject(new Error('offline')));
    const said = await within(item).findByText(/^Details could not load\./);
    expect(within(said).getByRole('button', { name: 'Reload' })).toBeInTheDocument();
    expect(item.querySelector('.ws-ph-notes')).toBeNull();
    expect(within(item).queryByText(/^A yes would/)).not.toBeInTheDocument();
    inOrder(item, ['Details could not load.', 'Not decided.']);
  });

  it('says the Workshop has changed, with Reload, when the site was deployed again, and asks no more', async () => {
    const load = vi.fn(() => Promise.reject(new SiteChanged()));
    const item = await openFrom('#part-b/name-a-snapshot', SNAPSHOT, data(), load);
    const said = await within(item).findByText(/^The Workshop has changed since this page opened\./);
    expect(within(said).getByRole('button', { name: 'Reload' })).toBeInTheDocument();
    await userEvent.click(rowButton(SNAPSHOT));
    await userEvent.click(rowButton(SNAPSHOT));
    await wait(10);
    expect(load).toHaveBeenCalledTimes(1);
  });

  it('says nothing of a failure on a row with nothing to show', async () => {
    const item = await openFrom('#part-a/core', 'The engine runs on its own', data(), () => Promise.reject(new Error('offline')));
    await wait(30);
    expect(within(item).queryByText(/could not load/)).not.toBeInTheDocument();
  });

  it('draws what it holds as text, never as HTML', async () => {
    const hostile = fixtureFacts(data());
    hostile.entries['name-a-snapshot'] = { suggest: { verdict: 'take', why: '<img src=x onerror=alert(1)> <b>bold</b>' } };
    const item = await openFrom('#part-b/name-a-snapshot', SNAPSHOT, data(), () => Promise.resolve(hostile));
    await within(item).findByText('Recommended: take');
    expect(item.querySelector('img')).toBeNull();
    expect(item).toHaveTextContent('<img src=x onerror=alert(1)> <b>bold</b>');
  });
});

describe('the id', () => {
  const written: string[] = [];
  const clipboard = (writeText: (text: string) => Promise<void>) => vi.stubGlobal('navigator', Object.assign(Object.create(navigator), { clipboard: { writeText } }));

  it('is shown in the monospace face with a "Copy id" button, on an open entry', async () => {
    const item = await openFrom('#part-b/create', 'Create and delete a computer');
    const id = item.querySelector('.ws-id code') as HTMLElement;
    expect(id).toHaveTextContent('create');
    expect(within(item).getByRole('button', { name: 'Copy id' })).toBeInTheDocument();
  });

  it('writes the id to the clipboard, says "Copied" for two seconds, then "Copy id" again', async () => {
    written.length = 0;
    clipboard(async (text) => { written.push(text); });
    const item = await openFrom('#part-b/name-a-snapshot', SNAPSHOT);
    await userEvent.click(within(item).getByRole('button', { name: 'Copy id' }));
    expect(written).toEqual(['name-a-snapshot']);
    expect(await within(item).findByRole('button', { name: 'Copied' })).toBeInTheDocument();
    await wait(1500);
    expect(within(item).getByRole('button', { name: 'Copied' })).toBeInTheDocument();
    await wait(700);
    expect(within(item).getByRole('button', { name: 'Copy id' })).toBeInTheDocument();
  });

  it('announces the change politely', async () => {
    clipboard(async () => undefined);
    const item = await openFrom('#part-b/create', 'Create and delete a computer');
    const status = item.querySelector('.ws-id [role="status"]') as HTMLElement;
    expect(status).toHaveAttribute('aria-live', 'polite');
    expect(status).toBeEmptyDOMElement();
    await userEvent.click(within(item).getByRole('button', { name: 'Copy id' }));
    await waitFor(() => expect(status).toHaveTextContent('Copied'));
  });

  it('keeps the width: both words are in the button, the one not shown hidden from sight and from a screen reader', async () => {
    clipboard(async () => undefined);
    const item = await openFrom('#part-b/create', 'Create and delete a computer');
    const button = within(item).getByRole('button', { name: 'Copy id' });
    const [shown, other] = [...button.querySelectorAll('span')];
    expect(shown).toHaveTextContent('Copy id');
    expect(other).toHaveTextContent('Copied');
    expect(other).toHaveAttribute('aria-hidden', 'true');
    await userEvent.click(button);
    const [now, then] = [...(await within(item).findByRole('button', { name: 'Copied' })).querySelectorAll('span')];
    expect(now).toHaveTextContent('Copied');
    expect(then).toHaveTextContent('Copy id');
  });

  it('is reachable by keyboard and copies on Enter', async () => {
    written.length = 0;
    clipboard(async (text) => { written.push(text); });
    const item = await openFrom('#part-b/create', 'Create and delete a computer');
    const button = within(item).getByRole('button', { name: 'Copy id' });
    expect(button).toHaveAttribute('type', 'button');
    button.focus();
    expect(button).toHaveFocus();
    await userEvent.keyboard('{Enter}');
    expect(written).toEqual(['create']);
  });

  it('selects the id for a hand copy when the clipboard refuses, and does not say "Copied"', async () => {
    clipboard(() => Promise.reject(new DOMException('denied', 'NotAllowedError')));
    const item = await openFrom('#part-b/create', 'Create and delete a computer');
    await userEvent.click(within(item).getByRole('button', { name: 'Copy id' }));
    await waitFor(() => expect(getSelection()?.toString()).toBe('create'));
    expect(within(item).getByRole('button', { name: 'Copy id' })).toBeInTheDocument();
    expect(within(item).queryByRole('button', { name: 'Copied' })).not.toBeInTheDocument();
    expect(item.querySelector('.ws-id [role="status"]')).toBeEmptyDOMElement();
  });

  it('selects the id too when the page has no clipboard at all', async () => {
    vi.stubGlobal('navigator', Object.assign(Object.create(navigator), { clipboard: undefined }));
    const item = await openFrom('#part-b/create', 'Create and delete a computer');
    await userEvent.click(within(item).getByRole('button', { name: 'Copy id' }));
    expect(getSelection()?.toString()).toBe('create');
    expect(within(item).queryByRole('button', { name: 'Copied' })).not.toBeInTheDocument();
  });

  it('is on every entry, and not on a suggestion, which has no id of the plan', async () => {
    const entry = await openFrom('#part-a/core', 'The engine runs on its own');
    expect(entry.querySelector('.ws-id')).not.toBeNull();
    const suggestion = await openFrom('#suggested/issue-30', 'Start a task from Slack');
    expect(suggestion.querySelector('.ws-id')).toBeNull();
    expect(within(suggestion).queryByRole('button', { name: /^Copy id|^Copied/ })).not.toBeInTheDocument();
    expect(screen.queryAllByRole('button', { name: 'Copy id' }).length).toBeGreaterThan(0);
  });
});

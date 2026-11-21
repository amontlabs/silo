// The History of an open row: its closed line, its list, the text before and after a change, the link of a
// replacement, the shallow line, and the chunk that holds the changes.
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { App } from './App';
import { SiteChanged } from './chunks';
import { buildDataFrom } from './data';
import type { HistorySource } from './data';
import { buildData, planFiles } from './fixtures';
import { EXAMPLE_MAP } from './map-example';
import type { Change, ChangeName } from './plan';
import { itemOf, mockApi, owner, renderApp, rowButton, wait } from './test-support';

const hash = (n: number) => n.toString(16).padStart(40, '0');
const change = (n: number, date: string, what: ChangeName[], extra: Partial<Change> = {}): Change => ({ commit: hash(n), date, what, ...extra });
const source = (entries: Record<string, Change[]>, shallow = false): HistorySource => ({ changes: new Map(Object.entries(entries)), shallow });

const CORE = 'The engine runs on its own';
const ZIG = 'Which language is the core written in?';

/** The row of `id` opened from the address, on a plan whose history is given. */
async function open(hashPath: string, name: string, history: HistorySource | null, loadHistory?: () => Promise<Record<string, Change[]>>) {
  cleanup();
  mockApi({ me: owner });
  window.history.replaceState(null, '', `/workshop${hashPath}`);
  const data = buildData(undefined, history);
  await renderApp(data, undefined, undefined, loadHistory);
  await waitFor(() => expect(rowButton(name)).toHaveAttribute('aria-expanded', 'true'));
  return { item: itemOf(name), data };
}

const historyButton = (item: HTMLElement) => within(item).getByRole('button', { name: /^History/ });
const lines = (item: HTMLElement) => [...item.querySelectorAll<HTMLElement>('.ws-h')];
const phrasesOf = (line: HTMLElement) => (line.querySelector('.ws-h-what') as HTMLElement).textContent;

describe('the closed line of History', () => {
  it('reads History, the last change and its date, then the number of changes when there are several', async () => {
    const { item } = await open('#part-a/core', CORE, source({
      core: [change(3, '2026-11-20T12:00:00Z', ['taken']), change(2, '2026-11-18T09:00:00Z', ['notes']), change(1, '2026-11-12T09:00:00Z', ['added'])],
    }));
    expect(historyButton(item)).toHaveTextContent('History · Decided, 20 Nov · 3 changes');
    expect(historyButton(item)).toHaveAttribute('aria-expanded', 'false');
    expect(lines(item)).toHaveLength(0);
  });

  it('says no number for a single change', async () => {
    const { item } = await open('#part-a/core', CORE, source({ core: [change(1, '2026-11-12T09:00:00Z', ['added'])] }));
    expect(historyButton(item)).toHaveTextContent(/^History · Added, 12 Nov$/);
  });

  it('puts the phrases of the last change on the line, joined by a middle dot', async () => {
    const { item } = await open('#part-a/core-in-zig', ZIG, source({ 'core-in-zig': [change(2, '2026-11-20T12:00:00Z', ['text', 'notes', 'taken']), change(1, '2026-11-01T12:00:00Z', ['added'])] }));
    expect(historyButton(item)).toHaveTextContent('History · Text changed · Notes edited · Decided, 20 Nov · 2 changes');
  });

  it('sits beside Notes in the same bar, and is the only toggle of an entry without notes', async () => {
    const { item } = await open('#part-a/core-in-zig', ZIG, source({ 'core-in-zig': [change(1, '2026-11-12T09:00:00Z', ['added'])] }));
    const bar = item.querySelector('.ws-extras-bar') as HTMLElement;
    expect([...bar.querySelectorAll('button')].map((button) => button.textContent)).toEqual(['Notes', 'History · Added, 12 Nov']);
    const alone = await open('#part-a/calls', 'Every call defined once', source({ calls: [change(1, '2026-11-12T09:00:00Z', ['added'])] }));
    expect([...(alone.item.querySelector('.ws-extras-bar') as HTMLElement).querySelectorAll('button')].map((button) => button.textContent)).toEqual(['History · Added, 12 Nov']);
  });

  it('is not there for an entry with no change, nor for a plan with no history', async () => {
    const { item } = await open('#part-a/core', CORE, source({ calls: [change(1, '2026-11-12T09:00:00Z', ['added'])] }));
    expect(within(item).queryByRole('button', { name: /^History/ })).not.toBeInTheDocument();
    const none = await open('#part-a/calls', 'Every call defined once', null);
    expect(none.item.querySelector('.ws-extras')).toBeNull();
    expect(none.data.shallow).toBe(false);
  });

  it('is there for an entry with no thread yet: the history needs none', async () => {
    mockApi({ me: owner });
    window.history.replaceState(null, '', '/workshop#part-a/core');
    const data = buildDataFrom(planFiles(), { map: EXAMPLE_MAP, history: source({ core: [change(1, '2026-11-12T09:00:00Z', ['added'])] }) });
    render(<App data={data} loadHistory={() => Promise.resolve({ core: [change(1, '2026-11-12T09:00:00Z', ['added'])] })} />);
    await waitFor(() => expect(rowButton(CORE)).toHaveAttribute('aria-expanded', 'true'));
    await userEvent.click(historyButton(itemOf(CORE)));
    await waitFor(() => expect(lines(itemOf(CORE))).toHaveLength(1));
  });
});

describe('the list of History', () => {
  const CHANGES = [
    change(4, '2026-11-20T12:00:00Z', ['taken']),
    change(3, '2026-11-18T09:00:00Z', ['needs', 'surfaces']),
    change(2, '2026-11-15T09:00:00Z', ['moved']),
    change(1, '2026-11-12T09:00:00Z', ['added']),
  ];

  it('is closed until asked for, then lists the changes in place, newest first, one line each', async () => {
    const { item } = await open('#part-a/core', CORE, source({ core: CHANGES }));
    expect(item.querySelector('.ws-h')).toBeNull();
    await userEvent.click(historyButton(item));
    expect(historyButton(item)).toHaveAttribute('aria-expanded', 'true');
    await waitFor(() => expect(lines(item)).toHaveLength(4));
    expect(lines(item).map((line) => line.querySelector('small')?.textContent)).toEqual(['20 Nov', '18 Nov', '15 Nov', '12 Nov']);
    expect(lines(item).map(phrasesOf)).toEqual(['Decided', 'Needs changed · Where it shows changed', 'Moved to another part', 'Added']);
    // In the open row, below its text: nothing above it moves.
    expect(item.querySelector('.ws-more-in')).toContainElement(lines(item)[0]);
    await userEvent.click(historyButton(item));
    expect(item.querySelector('.ws-h')).toBeNull();
    expect(historyButton(item)).toHaveTextContent('History · Decided, 20 Nov · 4 changes');
  });

  it('links each line to its commit on GitHub, in a new tab', async () => {
    const { item } = await open('#part-a/core', CORE, source({ core: CHANGES }));
    await userEvent.click(historyButton(item));
    await waitFor(() => expect(lines(item)).toHaveLength(4));
    const links = lines(item).map((line) => within(line).getByRole('link', { name: 'commit' }));
    expect(links.map((link) => link.getAttribute('href'))).toEqual(CHANGES.map((entry) => `https://github.com/amontlabs/silo/commit/${entry.commit}`));
    for (const link of links) {
      expect(link).toHaveAttribute('target', '_blank');
      expect(link.getAttribute('rel')).toContain('noopener');
    }
  });

  it('says each name of a change in the page\'s words, "Decided" on a decision and on a feature', async () => {
    const names: ChangeName[] = ['added', 'moved', 'sets', 'needs', 'keeps', 'surfaces', 'suggest', 'notes', 'taken', 'dropped', 'cleared', 'started', 'built'];
    const all = [change(2, '2026-11-20T12:00:00Z', names), change(1, '2026-11-12T09:00:00Z', ['added'])];
    const feature = await open('#part-a/core', CORE, source({ core: all }));
    await userEvent.click(historyButton(feature.item));
    await waitFor(() => expect(lines(feature.item)).toHaveLength(2));
    expect(phrasesOf(lines(feature.item)[0])).toBe('Added · Moved to another part · What a yes would do changed · Needs changed · What it must keep changed · Where it shows changed · Recommendation changed · Notes edited · Decided · Dropped · Back to not decided · Being built · Built');
    const decision = await open('#part-a/core-in-zig', ZIG, source({ 'core-in-zig': all }));
    await userEvent.click(historyButton(decision.item));
    await waitFor(() => expect(lines(decision.item)).toHaveLength(2));
    expect(phrasesOf(lines(decision.item)[0])).toContain('Notes edited · Decided · Dropped');
    // No name of a key ever reaches the page.
    expect(decision.item.textContent).not.toMatch(/\b(taken|sets|keeps|surfaces|suggest|cleared|started|planned)\b/);
  });

  it('opens the title and the body before and after for "Text changed", and closes them again', async () => {
    const text = { before: { title: 'Create a computer', body: 'Make one.' }, after: { title: 'Create and delete a computer', body: 'Make a Linux computer.' } };
    const fetched = vi.spyOn(globalThis, 'fetch');
    const { item } = await open('#part-b/create', 'Create and delete a computer', source({ create: [change(2, '2026-11-20T12:00:00Z', ['text', 'notes'], { text }), change(1, '2026-11-12T09:00:00Z', ['added'])] }));
    const before = fetched.mock.calls.length;
    await userEvent.click(historyButton(item));
    const button = await within(item).findByRole('button', { name: 'Text changed' });
    expect(item.querySelector('.ws-h-diff')).toBeNull();
    await userEvent.click(button);
    expect(button).toHaveAttribute('aria-expanded', 'true');
    const [was, now] = [...(item.querySelector('.ws-h-diff') as HTMLElement).children] as HTMLElement[];
    expect(was).toHaveTextContent('BeforeCreate a computerMake one.');
    expect(now).toHaveTextContent('AfterCreate and delete a computerMake a Linux computer.');
    // The texts come with the chunk: nothing is fetched for them.
    expect(fetched.mock.calls.slice(before).map(([input]) => String(input)).filter((url) => !url.includes('/api/workshop/'))).toEqual([]);
    await userEvent.click(button);
    expect(item.querySelector('.ws-h-diff')).toBeNull();
  });

  it('shows only the part that differs: the body alone when the title is the same, a missing body as nothing', async () => {
    const same = { before: { title: 'Fork a computer', body: 'Copy it.' }, after: { title: 'Fork a computer', body: 'Copy it, whole.' } };
    const added = { before: { title: 'Run a command', body: null }, after: { title: 'Run a command', body: 'Run it inside.' } };
    const a = await open('#part-d/fork', 'Fork a computer', source({ fork: [change(1, '2026-11-20T12:00:00Z', ['text'], { text: same })] }));
    await userEvent.click(historyButton(a.item));
    await userEvent.click(await within(a.item).findByRole('button', { name: 'Text changed' }));
    const [was, now] = [...(a.item.querySelector('.ws-h-diff') as HTMLElement).children] as HTMLElement[];
    expect(was).toHaveTextContent(/^BeforeCopy it\.$/);
    expect(now).toHaveTextContent(/^AfterCopy it, whole\.$/);
    const b = await open('#part-b/exec', 'Run a command', source({ exec: [change(1, '2026-11-20T12:00:00Z', ['text'], { text: added })] }));
    await userEvent.click(historyButton(b.item));
    await userEvent.click(await within(b.item).findByRole('button', { name: 'Text changed' }));
    const [none, some] = [...(b.item.querySelector('.ws-h-diff') as HTMLElement).children] as HTMLElement[];
    expect(none).toHaveTextContent(/^Before$/);
    expect(some).toHaveTextContent(/^AfterRun it inside\.$/);
  });

  it('links "Replaced by" to the question of the decision that replaced it, and opens it', async () => {
    const { item } = await open('#product/plain-logs', 'Which logs does Silo keep?', source({
      'plain-logs': [change(2, '2026-11-20T12:00:00Z', ['replaced'], { by: 'signed-logs' }), change(1, '2026-11-01T12:00:00Z', ['added'])],
    }));
    await userEvent.click(historyButton(item));
    const line = (await waitFor(() => { const found = lines(item); expect(found).toHaveLength(2); return found; }))[0];
    expect(phrasesOf(line)).toBe('Replaced by How are logs kept honest?');
    const link = within(line).getByRole('link', { name: 'How are logs kept honest?' });
    expect(link).toHaveAttribute('href', '#part-e/signed-logs');
    await userEvent.click(link);
    await waitFor(() => expect(rowButton('How are logs kept honest?')).toHaveAttribute('aria-expanded', 'true'));
  });

  it('ends with the line "Earlier changes are not available in this build." when the clone was shallow, and only then', async () => {
    const shallow = await open('#part-a/core', CORE, source({ core: CHANGES }, true));
    await userEvent.click(historyButton(shallow.item));
    await waitFor(() => expect(lines(shallow.item)).toHaveLength(4));
    const list = shallow.item.querySelector('#entry-core-history') as HTMLElement;
    expect(list.lastElementChild).toHaveTextContent('Earlier changes are not available in this build.');
    const whole = await open('#part-a/calls', 'Every call defined once', source({ calls: CHANGES }));
    await userEvent.click(historyButton(whole.item));
    await waitFor(() => expect(lines(whole.item)).toHaveLength(4));
    expect(whole.item.textContent).not.toContain('Earlier changes');
  });
});

describe('the chunk of changes', () => {
  const only = source({ core: [change(2, '2026-11-20T12:00:00Z', ['taken']), change(1, '2026-11-12T09:00:00Z', ['added'])] });

  it('is not loaded with the page, nor with an open row: only when a History is reached or opened, once', async () => {
    const load = vi.fn(() => Promise.resolve({ core: only.changes.get('core') as Change[] }));
    const { item } = await open('#part-a/core', CORE, only, load);
    await wait(30);
    expect(load).not.toHaveBeenCalled();
    await userEvent.click(historyButton(item));
    await waitFor(() => expect(lines(item)).toHaveLength(2));
    await userEvent.click(historyButton(item));
    await userEvent.click(historyButton(item));
    expect(load).toHaveBeenCalledTimes(1);
  });

  it('starts when a pointer or the focus reaches the History, before it is opened', async () => {
    const load = vi.fn(() => Promise.resolve({ core: only.changes.get('core') as Change[] }));
    const { item } = await open('#part-a/core', CORE, only, load);
    await userEvent.hover(historyButton(item));
    await waitFor(() => expect(load).toHaveBeenCalledTimes(1));
    expect(item.querySelector('.ws-h')).toBeNull();
  });

  it('holds a placeholder line for each change while it loads, then the lines in their place', async () => {
    let release: (changes: Record<string, Change[]>) => void = () => {};
    const load = () => new Promise<Record<string, Change[]>>((resolve) => { release = resolve; });
    const { item } = await open('#part-a/core', CORE, only, load);
    await userEvent.click(historyButton(item));
    const panel = item.querySelector('#entry-core-history') as HTMLElement;
    expect(panel.querySelectorAll('.ws-h-ghost')).toHaveLength(2);
    expect(panel.querySelector('[aria-busy="true"]')).not.toBeNull();
    expect(panel.querySelector('.ws-h')).toBeNull();
    release({ core: only.changes.get('core') as Change[] });
    await waitFor(() => expect(lines(item)).toHaveLength(2));
    expect(panel.querySelector('.ws-h-ghost')).toBeNull();
    expect(panel.querySelector('[aria-busy]')).toBeNull();
  });

  it('says it could not load, and loads again at the next opening', async () => {
    const load = vi.fn()
      .mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValue({ core: only.changes.get('core') as Change[] });
    const { item } = await open('#part-a/core', CORE, only, load);
    // A click alone, with no pointer reaching the button first.
    fireEvent.click(historyButton(item));
    expect(await within(item).findByText('History could not load.')).toBeInTheDocument();
    fireEvent.click(historyButton(item));
    fireEvent.click(historyButton(item));
    await waitFor(() => expect(lines(item)).toHaveLength(2));
    expect(load).toHaveBeenCalledTimes(2);
  });

  it('keeps a failure as it is under a pointer passing over, and asks again only at an opening', async () => {
    const load = vi.fn().mockRejectedValue(new Error('offline'));
    const { item } = await open('#part-a/core', CORE, only, load);
    fireEvent.click(historyButton(item));
    expect(await within(item).findByText('History could not load.')).toBeInTheDocument();
    for (const fire of [fireEvent.pointerEnter, fireEvent.pointerDown, fireEvent.touchStart, fireEvent.focus]) fire(historyButton(item));
    fireEvent.click(historyButton(item));
    expect(load).toHaveBeenCalledTimes(1);
  });

  it('says the Workshop has changed when the site was deployed again, offers to reload, and never asks again', async () => {
    const load = vi.fn().mockRejectedValue(new SiteChanged());
    const { item } = await open('#part-a/core', CORE, only, load);
    fireEvent.click(historyButton(item));
    expect(await within(item).findByText(/The Workshop has changed since this page opened/)).toBeInTheDocument();
    expect(within(item).getByRole('button', { name: 'Reload' })).toBeInTheDocument();
    fireEvent.click(historyButton(item));
    fireEvent.click(historyButton(item));
    expect(load).toHaveBeenCalledTimes(1);
  });
});

describe('the data of the page', () => {
  it('holds for each entry only its last change and their number, never the texts', () => {
    const text = { before: { title: 'OLD TITLE', body: 'OLD BODY' }, after: { title: 'NEW TITLE', body: 'NEW BODY' } };
    const data = buildData(undefined, source({ core: [change(2, '2026-11-20T12:00:00Z', ['text', 'notes'], { text }), change(1, '2026-11-12T09:00:00Z', ['added'])] }));
    expect(data.entries.find((entry) => entry.id === 'core')?.history).toEqual({ date: '2026-11-20T12:00:00Z', what: ['text', 'notes'], count: 2 });
    expect(data.entries.find((entry) => entry.id === 'calls')?.history).toBeNull();
    expect(JSON.stringify(data)).not.toMatch(/OLD TITLE|NEW BODY|commit/);
  });
});

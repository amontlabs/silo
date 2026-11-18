// Entries opening independently, late answers, addresses, focus and the Map/Order swap.
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it, vi } from 'vitest';
import { App, OPEN_GUARD_MS } from './App';
import { buildData, issue, liveState } from './fixtures';
import {
  clock, detail, itemOf, mockApi, ok, owner, ownerOn, part, posts, refused, renderApp, rowButton, visitor,
} from './test-support';

describe('entries open independently', () => {
  it('keeps every open row open, and a row closes only itself', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#computers');
    await renderApp();
    history.replaceState(null, '', '/workshop#engine');
    act(() => { dispatchEvent(new PopStateEvent('popstate')); });
    await userEvent.click(rowButton('The engine runs on its own'));
    await userEvent.click(rowButton('Every call defined once'));
    const open = () => [...document.querySelectorAll('.ws-item.open')].map((item) => item.id);
    expect(open()).toEqual(['entry-engine', 'entry-calls']);
    expect(location.hash).toBe('#engine/calls');
    await userEvent.click(rowButton('The engine runs on its own'));
    expect(open()).toEqual(['entry-calls']);
    expect(location.hash).toBe('#engine/calls');
  });
});

describe('comments that arrive late', () => {
  it('hold one place per expected comment while loading, with no loading line, and load on hover', async () => {
    let release: () => void = () => {};
    const calls = mockApi({ routes: { 'GET issue': (url) => new Promise((resolve) => { release = () => resolve(ok(detail(Number(url.searchParams.get('n')), { comments: [1, 2].map((id) => ({ id, author: 'mara-k', avatar: '', bot: false, createdAt: '2026-11-14T10:00:00Z', text: `Comment ${id}.` })) }))); }) } });
    history.replaceState(null, '', '/workshop#computers');
    await renderApp();
    fireEvent.pointerEnter(rowButton('Create and delete a computer'));
    expect(calls.filter((call) => call.path === 'issue')).toHaveLength(1);
    await userEvent.click(rowButton('Create and delete a computer'));
    // The hover's fetch serves the opening.
    expect(calls.filter((call) => call.path === 'issue')).toHaveLength(1);
    const item = itemOf('Create and delete a computer');
    expect(item.querySelectorAll('.ws-c-ghost')).toHaveLength(2);
    expect(within(item).queryByText(/Loading/)).not.toBeInTheDocument();
    await act(async () => release());
    expect(item.querySelectorAll('.ws-c-ghost')).toHaveLength(0);
    expect(item.querySelectorAll('.ws-c')).toHaveLength(2);
  });

  it('are fetched again on every opening, and a failed load is tried again', async () => {
    let fail = true;
    const calls = mockApi({ routes: { 'GET issue': (url) => (fail ? refused(502, 'GitHub failed.') : ok(detail(Number(url.searchParams.get('n'))))) } });
    const time = clock();
    history.replaceState(null, '', '/workshop#computers');
    await renderApp();
    const fetches = () => calls.filter((call) => call.path === 'issue').length;
    await userEvent.click(rowButton('Run a command'));
    expect(await screen.findByText('Comments could not load.')).toBeInTheDocument();
    const before = fetches();
    fail = false;
    await userEvent.click(rowButton('Run a command'));
    time.advance(10_000);
    await userEvent.click(rowButton('Run a command'));
    await waitFor(() => expect(screen.queryByText('Comments could not load.')).not.toBeInTheDocument());
    expect(fetches()).toBe(before + 1);
    await userEvent.click(rowButton('Run a command'));
    time.advance(10_000);
    await userEvent.click(rowButton('Run a command'));
    await waitFor(() => expect(fetches()).toBe(before + 2));
  });
});

describe('the first answers', () => {
  it('keep an entry named by the address closed until the live state is there, then open it', async () => {
    let release: (value: { status: number; body: unknown }) => void = () => {};
    mockApi({ state: () => new Promise((resolve) => { release = resolve; }) });
    history.replaceState(null, '', '/workshop#computers/exec');
    render(<App data={buildData()} />);
    expect(screen.getByRole('heading', { level: 2, name: 'Computers' })).toBeInTheDocument();
    expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'false');
    expect(location.hash).toBe('#computers/exec');
    await act(async () => release(ok(liveState())));
    await waitFor(() => expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true'));
  });
});

describe('the owner\'s verdict buttons', () => {
  it('each have their own guard: an earlier open row stays armed when another opens', async () => {
    const time = clock();
    const calls = mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#computers/computers');
    await renderApp();
    await ownerOn('Create and delete a computer');
    const take = (name: string) => within(itemOf(name)).getByRole('button', { name: /^Take/ });
    expect(take('Create and delete a computer')).toHaveClass('ws-unarmed');
    await act(async () => { time.advance(OPEN_GUARD_MS); await new Promise((resolve) => setTimeout(resolve, OPEN_GUARD_MS + 20)); });
    expect(take('Create and delete a computer')).not.toHaveClass('ws-unarmed');
    // Opening a second row starts that row's guard only.
    await userEvent.click(rowButton('Run a command'));
    expect(take('Run a command')).toHaveClass('ws-unarmed');
    expect(take('Create and delete a computer')).not.toHaveClass('ws-unarmed');
    await userEvent.click(take('Run a command'));
    expect(posts(calls, 'verdict')).toHaveLength(0);
    await userEvent.click(take('Create and delete a computer'));
    expect(posts(calls, 'verdict').map((call) => call.body)).toEqual([{ n: 12, verdict: 'take' }]);
  });

  it('take Enter on the open row that holds the focus, else on the one opened last', async () => {
    const time = clock();
    const calls = mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#computers/computers');
    await renderApp();
    await ownerOn('Create and delete a computer');
    await userEvent.click(rowButton('Run a command'));
    time.advance(OPEN_GUARD_MS);
    // The focus is on the earlier row: Enter is its verdict, and it does not close it.
    act(() => rowButton('Create and delete a computer').focus());
    fireEvent.keyDown(rowButton('Create and delete a computer'), { key: 'Enter' });
    expect(posts(calls, 'verdict').map((call) => call.body)).toEqual([{ n: 12, verdict: 'take' }]);
    // With the focus on the page, the row opened last; a held key does nothing.
    act(() => (document.activeElement as HTMLElement).blur());
    fireEvent.keyDown(document.body, { key: 'Enter', repeat: true });
    expect(posts(calls, 'verdict')).toHaveLength(1);
    await waitFor(() => expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true'));
    // The row that became current is armed again, like a freshly opened one.
    act(() => (document.activeElement as HTMLElement).blur());
    fireEvent.keyDown(document.body, { key: 'Enter' });
    expect(posts(calls, 'verdict')).toHaveLength(1);
    time.advance(OPEN_GUARD_MS);
    fireEvent.keyDown(document.body, { key: 'Enter' });
    expect(posts(calls, 'verdict').map((call) => call.body?.n)).toEqual([12, 13]);
  });

});

describe('addresses', () => {
  it('open a dropped entry in the left-out list where it was opened', async () => {
    mockApi({ state: () => ok(liveState({ 13: issue({ entry: 'exec', verdict: 'drop', state: 'closed', reason: 'not_planned' }) })) });
    history.replaceState(null, '', '/workshop#left');
    await renderApp();
    await userEvent.click(rowButton('Run a command'));
    expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByRole('heading', { level: 2, name: 'Left out' })).toBeInTheDocument();
    expect(location.hash).toBe('#left/exec');
    act(() => { history.replaceState(null, '', '/workshop#left/exec'); dispatchEvent(new PopStateEvent('popstate')); });
    expect(screen.getByRole('heading', { level: 2, name: 'Left out' })).toBeInTheDocument();
  });

  it('open the area when its entry no longer exists, and find an entry under another area', async () => {
    history.replaceState(null, '', '/workshop#engine/gone-away');
    mockApi();
    const first = await renderApp();
    expect(screen.getByRole('heading', { level: 2, name: 'Engine' })).toBeInTheDocument();
    expect(location.hash).toBe('#engine');
    first.unmount();
    history.replaceState(null, '', '/workshop#forks/exec');
    await renderApp();
    await waitFor(() => expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true'));
    expect(location.hash).toBe('#computers/exec');
  });
});

describe('focus', () => {
  it('returns to the link that opened the left-out list, the latest, the suggestions or the decisions', async () => {
    mockApi({ me: owner });
    await renderApp();
    for (const [name, title] of [['Left out · 2', 'Left out'], ['Latest', 'Latest'], ['2 suggested', 'Suggest a feature'], ['Decisions', 'Decisions']]) {
      await userEvent.click(screen.getByRole('link', { name }));
      expect(screen.getByRole('heading', { level: 2, name: title })).toHaveFocus();
      fireEvent.keyDown(document.activeElement as Element, { key: 'Escape' });
      await waitFor(() => expect(screen.getByRole('link', { name })).toHaveFocus());
    }
  });
});

describe('state classes', () => {
  it('puts the classes of the Workshop\'s states on no other element', () => {
    // A state class is set on .ws alone (App.tsx), and workshop.css styles it there. Another element carrying the same
    // name would take the rules written for the whole Workshop, or give its own rules to the Workshop.
    const dir = join(process.cwd(), 'src/workshop');
    const app = readFileSync(join(dir, 'App.tsx'), 'utf8');
    const begin = app.indexOf('const classes = [');
    const statement = app.slice(begin, app.indexOf('].filter(Boolean)', begin));
    // A name built from a state (`v-${view}`) takes each value of that state's type, read from its useState.
    const valuesOf = (name: string) => {
      let type = app.match(new RegExp(`const \\[${name}, \\w+\\] = useState<([^>]+)>`))?.[1] ?? '';
      if (/^\w+$/.test(type)) type = app.match(new RegExp(`type ${type} = ([^;]+);`))?.[1] ?? '';
      const values = [...type.matchAll(/'([a-z][\w-]*)'/g)].map((match) => match[1]);
      expect(values, `the values of ${name}`).not.toEqual([]);
      return values;
    };
    // Every item of the list is read: a name, a name built from a state, or either one behind a condition.
    const nameOf = (text: string): string[] => {
      const plain = text.match(/^'([a-z][\w-]*)'$/);
      if (plain) return [plain[1]];
      const built = text.match(/^`([a-z][\w-]*)\$\{(\w+)\}([\w-]*)`$/);
      if (built) return valuesOf(built[2]).map((value) => built[1] + value + built[3]);
      throw new Error(`a state class this check cannot read: ${text}`);
    };
    const items = statement.slice('const classes = ['.length).split(',').map((item) => item.trim()).filter(Boolean);
    const states = new Set(items.flatMap((item) => {
      const branches = item.match(/^[\w.!&| ]+\?\s*(.+?)\s*:\s*(.+)$/);
      return (branches ? [branches[1], branches[2]] : [item]).filter((text) => text !== "''").flatMap(nameOf);
    }));
    states.delete('ws');
    expect(states).toContain('v-map');
    expect(states).toContain('swap-out');
    // The same state on purpose: the account slot in the header fades in with the person's details, like the Workshop.
    const shared = "accountSlot.classList.toggle('ws-arr-p'";
    expect(app).toContain(shared);
    const walk = (folder: string): string[] => readdirSync(folder, { withFileTypes: true }).flatMap((entry) =>
      entry.isDirectory() ? walk(join(folder, entry.name)) : [join(folder, entry.name)]);
    const site = join(process.cwd(), 'src');
    const sources = [
      ...walk(site).filter((file) => /\.(tsx?|html)$/.test(file) && !/\.test\.|test-|fixtures|devFixture/.test(file)),
      join(process.cwd(), 'index.html'),
      join(process.cwd(), 'workshop/index.html'),
    ];
    const elsewhere = sources.flatMap((file) => {
      const code = readFileSync(file, 'utf8').replace(statement, '').replace(shared, '').replace(/\/\*[\s\S]*?\*\/|\/\/.*$/gm, '');
      // A class name stands between quotes, spaces or a dot, wherever it is written: in a template, after text, in a selector.
      return [...states].filter((name) => new RegExp(`(^|[\\s'"\`.])${name}(?=[\\s'"\`.:>)\\]]|$)`, 'm').test(code))
        .map((name) => `${file.slice(process.cwd().length + 1)}: ${name}`);
    });
    expect(elsewhere).toEqual([]);
  });
});

describe('Map and Order', () => {
  it('end switched and visible when a part is clicked during the swap', async () => {
    vi.stubGlobal('matchMedia', (query: string) => ({ matches: query.includes('min-width'), media: query, addEventListener() {}, removeEventListener() {} }));
    mockApi();
    const { container } = await renderApp();
    await userEvent.click(screen.getByRole('button', { name: 'Order' }));
    await userEvent.click(part('forks'));
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 600)); });
    const root = container.querySelector('.ws') as HTMLElement;
    expect(root).not.toHaveClass('swap-out');
    expect(root).toHaveClass('v-order');
  });
});

describe('words', () => {
  it('say what a visitor and the owner need', async () => {
    mockApi({ me: visitor, routes: { 'POST logout': () => refused(502, 'GitHub failed.') } });
    history.replaceState(null, '', '/workshop#computers/exec');
    await renderApp();
    expect(await screen.findByRole('textbox', { name: 'Comment' })).toHaveAttribute('placeholder', 'Add a comment');
    await userEvent.click(screen.getByRole('button', { name: 'Sign out' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(/^Could not sign out\.$/);
  });

  it('give a signed-out visitor a reason on the thumbs-up, and say offline plainly', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#computers');
    const first = await renderApp();
    expect(within(itemOf('Run a command')).getByRole('link', { name: /4 votes/ })).toHaveAttribute('title', 'Sign in to vote');
    first.unmount();
    // A first visit: nothing remembered.
    localStorage.clear();
    mockApi({ state: () => refused(503, 'not configured') });
    history.replaceState(null, '', '/workshop#suggested');
    await renderApp();
    expect(screen.getByText('Suggestions are offline.')).toBeInTheDocument();
    expect(screen.getAllByText('Votes and comments are offline.').length).toBeGreaterThan(0);
  });
});

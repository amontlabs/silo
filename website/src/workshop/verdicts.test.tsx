// The owner's Take and Drop on an open entry: who sees them, the press that arms and the press that sends, the
// pending state, the verdict shown at once and kept across a reload until a build carries it, and a refusal.
import { act, cleanup, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { apiConfig } from './api';
import type { Me } from './api';
import { ARM_MS, CONFIRM_AFTER_MS } from './Entry';
import { buildData, DECISIONS, fixtureFacts } from './fixtures';
import { KEEP_FIELDS_MS } from './writes';
import type { BuildData } from './plan';
import { clock, mockApi, ok, owner, posts, refused, renderApp, rowButton, visitor, withheldTimers } from './test-support';
import type { Reply } from './test-support';

const BUILT = 'c'.repeat(40);
const theOwner: Me = { ...owner, owner: true };
const NAME = 'Create and delete a computer';

/** The fixture plan built from commit BUILT; the decision `one-way` has the verdict the agents wrote, when `agents` gives one. */
function built(agents?: 'take' | 'drop'): BuildData {
  const decisions = agents ? DECISIONS.map((decision) => (decision.id === 'one-way' ? { ...decision, verdict: agents, by: 'agents' as const } : decision)) : DECISIONS;
  return { ...buildData({ decisions }), builtFrom: BUILT };
}

let key = 'create';

/** The page with an entry opened (`create` unless `at` names another), as `me`, with `verdict` answering the owner's verdict. */
async function openCreate(data: BuildData, me: Me | null, verdict: () => Reply = () => ok({ ok: true, commit: 'e'.repeat(40) }), at = { key: 'create', hash: '#part-b/create', name: NAME }) {
  key = at.key;
  const calls = mockApi({ me, routes: { 'POST verdict': verdict } });
  history.replaceState(null, '', `/workshop${at.hash}`);
  await renderApp(data);
  await waitFor(() => expect(rowButton(at.name)).toHaveAttribute('aria-expanded', 'true'));
  return calls;
}

const item = () => document.getElementById(`entry-${key}`) as HTMLElement;
/** The second press, the confirm, made a moment after the first as a person makes it. */
function confirm(name: RegExp) {
  vi.spyOn(Date, 'now').mockReturnValue(Date.now() + CONFIRM_AFTER_MS);
  fireEvent.click(button(name) as HTMLElement);
}
const button = (name: RegExp) => within(item()).queryByRole('button', { name });
const stateLine = () => item().querySelector('.ws-state > span')?.textContent;

describe('the owner\'s verdict', () => {
  it('shows Take and Drop to the owner alone, once me says so, and only on a build that names its commit', async () => {
    for (const [me, data, shown] of [[theOwner, built(), true], [visitor, built(), false], [owner, built(), false], [null, built(), false], [theOwner, { ...built(), builtFrom: null }, false]] as const) {
      await openCreate(data, me);
      if (shown) await waitFor(() => expect(button(/^Take$/)).not.toBeNull());
      else await waitFor(() => expect(document.querySelector('.ws-account, .ws-account-inline a')).not.toBeNull());
      expect(button(/^Take$/) !== null, `${me?.login ?? 'nobody'}, built from ${data.builtFrom}`).toBe(shown);
      expect(button(/^Drop$/) !== null).toBe(shown);
      cleanup();
      localStorage.clear();
    }
  });

  it('holds the room of the buttons for the owner remembered until me answers, then draws them in it, or nothing for another', async () => {
    for (const [answered, shown] of [[theOwner, true], [visitor, false]] as const) {
      localStorage.setItem('silo-workshop-me', JSON.stringify({ login: 'polarzero', avatar: '', owner: true }));
      let answerMe: (reply: Reply) => void = () => {};
      mockApi({ me: theOwner, routes: { 'GET me': () => new Promise((resolve) => { answerMe = resolve; }) } });
      history.replaceState(null, '', '/workshop#part-b/create');
      await renderApp(built());
      await waitFor(() => expect(rowButton(NAME)).toHaveAttribute('aria-expanded', 'true'));
      const held = item().querySelector('.ws-verdict.ws-held');
      expect(held).not.toBeNull();
      expect(held).toHaveAttribute('aria-hidden', 'true');
      expect(button(/^Take$/)).toBeNull();
      await act(async () => answerMe(ok(answered)));
      await waitFor(() => expect(item().querySelector('.ws-verdict.ws-held')).toBeNull());
      expect(button(/^Take$/) !== null).toBe(shown);
      cleanup();
      localStorage.clear();
    }
  });

  it('offers no live Take or Drop before the entry\'s details show: what a yes would change is part of what the owner decides on', async () => {
    const plain = buildData();
    const data = { ...plain, builtFrom: BUILT };
    let giveFacts: () => void = () => {};
    const facts = new Promise<ReturnType<typeof fixtureFacts>>((resolve) => { giveFacts = () => resolve(fixtureFacts(plain)); });
    mockApi({ me: theOwner });
    history.replaceState(null, '', '/workshop#part-b/create');
    await renderApp(data, undefined, () => facts);
    await waitFor(() => expect(rowButton(NAME)).toHaveAttribute('aria-expanded', 'true'));
    // me has answered for the owner; the details are still loading: the buttons' room is held, inert.
    await waitFor(() => expect(item().querySelector('.ws-verdict')).not.toBeNull());
    expect(item().querySelector('.ws-ph-notes')).not.toBeNull();
    expect(button(/^Take$/)).toBeNull();
    expect(item().querySelector('.ws-verdict')).toHaveAttribute('aria-hidden', 'true');
    await act(async () => giveFacts());
    await waitFor(() => expect(button(/^Take$/)).not.toBeNull());
    expect(item().querySelector('.ws-ph-notes')).toBeNull();
    expect(within(item()).getByText('Recommended: take')).toBeInTheDocument();
  });

  it('arms on the first press and sends on the second: the id, the verdict and the build, nothing else', async () => {
    const calls = await openCreate(built(), theOwner);
    await waitFor(() => expect(button(/^Take$/)).not.toBeNull());
    fireEvent.click(button(/^Take$/) as HTMLElement);
    expect(button(/^Confirm take$/)).toHaveTextContent(/^Confirm/);
    expect(posts(calls, 'verdict')).toHaveLength(0);
    // Pressing the other arms it instead.
    fireEvent.click(button(/^Drop$/) as HTMLElement);
    expect(button(/^Take$/)).not.toBeNull();
    expect(button(/^Confirm drop$/)).not.toBeNull();
    confirm(/^Confirm drop$/);
    await waitFor(() => expect(posts(calls, 'verdict')).toHaveLength(1));
    expect(posts(calls, 'verdict')[0].body).toEqual({ entry: 'create', verdict: 'drop', built: BUILT });
    expect(posts(calls, 'verdict')[0].headers).toMatchObject({ 'X-Workshop': '1' });
  });

  it('a held key never confirms: its repeats are ignored, and so is a press sooner than a moment after arming', async () => {
    const time = clock();
    const calls = await openCreate(built(), theOwner);
    await waitFor(() => expect(button(/^Take$/)).not.toBeNull());
    // Enter held on Take: the first keydown's click arms it; Chromium clicks again on each repeat of the key.
    fireEvent.click(button(/^Take$/) as HTMLElement);
    const armed = button(/^Confirm take$/) as HTMLElement;
    time.advance(CONFIRM_AFTER_MS * 10);
    expect(fireEvent.keyDown(armed, { key: 'Enter', repeat: true }), 'the repeat\'s default, its click, is prevented').toBe(false);
    fireEvent.click(armed);
    expect(posts(calls, 'verdict')).toHaveLength(0);
    expect(button(/^Confirm take$/)).not.toBeNull();
    // The key let go, a new press confirms.
    fireEvent.keyUp(armed, { key: 'Enter' });
    fireEvent.keyDown(armed, { key: 'Enter' });
    fireEvent.click(armed);
    await waitFor(() => expect(posts(calls, 'verdict')).toHaveLength(1));
    cleanup();
    localStorage.clear();

    // A second press sooner than CONFIRM_AFTER_MS after arming is the same gesture: ignored, and the button stays armed.
    const again = await openCreate(built(), theOwner);
    await waitFor(() => expect(button(/^Drop$/)).not.toBeNull());
    fireEvent.click(button(/^Drop$/) as HTMLElement);
    time.advance(CONFIRM_AFTER_MS - 1);
    fireEvent.click(button(/^Confirm drop$/) as HTMLElement);
    expect(posts(again, 'verdict')).toHaveLength(0);
    time.advance(1);
    fireEvent.click(button(/^Confirm drop$/) as HTMLElement);
    await waitFor(() => expect(posts(again, 'verdict')).toHaveLength(1));
  });

  it('an armed button goes back after a while without a second press', async () => {
    const timers = withheldTimers(ARM_MS);
    const calls = await openCreate(built(), theOwner);
    await waitFor(() => expect(button(/^Take$/)).not.toBeNull());
    fireEvent.click(button(/^Take$/) as HTMLElement);
    expect(button(/^Confirm take$/)).not.toBeNull();
    await timers.fire();
    expect(button(/^Take$/)).not.toBeNull();
    fireEvent.click(button(/^Take$/) as HTMLElement);
    expect(posts(calls, 'verdict')).toHaveLength(0);
    // Left armed at the end: the page's own timer is cleared as it unmounts.
  });

  it('is pending while the server writes, then shows the verdict at once, kept across reloads and builds until a built plan carries it', async () => {
    let answer: (reply: Reply) => void = () => {};
    const data = built();
    await openCreate(data, theOwner, () => new Promise((resolve) => { answer = resolve; }));
    await waitFor(() => expect(button(/^Take$/)).not.toBeNull());
    expect(stateLine()).toBe('Not decided.');
    fireEvent.click(button(/^Take$/) as HTMLElement);
    confirm(/^Confirm take$/);
    const sending = await waitFor(() => button(/^Taking…$/) as HTMLElement);
    expect(sending).toBeDisabled();
    expect(sending).toHaveAttribute('aria-busy', 'true');
    expect(button(/^Drop$/)).toBeDisabled();
    expect(stateLine()).toBe('Not decided.');
    await act(async () => answer(ok({ ok: true, commit: 'e'.repeat(40) })));
    await waitFor(() => expect(stateLine()).toBe('Decided.'));
    expect(button(/^Take$/)).toBeNull();
    expect(screen.getByText('Taken')).toBeInTheDocument();

    // A reload of the same build still shows it.
    cleanup();
    await openCreate(data, theOwner);
    expect(stateLine()).toBe('Decided.');
    cleanup();
    // A build that does not carry it yet (the deploy of another push went live first) still shows it.
    await openCreate({ ...data, builtFrom: 'd'.repeat(40) }, theOwner);
    expect(stateLine()).toBe('Decided.');
    cleanup();
    // A build whose plan carries it shows it from the plan, and the page forgets its own: a build without it, an
    // older one served again, shows its plan alone.
    const carried = { ...data, builtFrom: 'f'.repeat(40), entries: data.entries.map((entry) => (entry.id === 'create' ? { ...entry, verdict: 'take' as const, by: null } : entry)) };
    await openCreate(carried, theOwner);
    expect(stateLine()).toBe('Decided.');
    cleanup();
    await openCreate({ ...data, builtFrom: 'a'.repeat(40) }, theOwner);
    expect(stateLine()).toBe('Not decided.');
  });

  it('forgets a verdict no build carried after a day', async () => {
    const time = clock();
    const data = built();
    await openCreate(data, theOwner);
    await waitFor(() => expect(button(/^Take$/)).not.toBeNull());
    fireEvent.click(button(/^Take$/) as HTMLElement);
    time.advance(CONFIRM_AFTER_MS);
    fireEvent.click(button(/^Confirm take$/) as HTMLElement);
    await waitFor(() => expect(stateLine()).toBe('Decided.'));
    cleanup();
    time.advance(KEEP_FIELDS_MS - 1);
    await openCreate({ ...data, builtFrom: 'd'.repeat(40) }, theOwner);
    expect(stateLine()).toBe('Decided.');
    cleanup();
    time.advance(1);
    await openCreate({ ...data, builtFrom: 'd'.repeat(40) }, theOwner);
    expect(stateLine()).toBe('Not decided.');
  });

  it('on an entry the agents decided, the owner\'s verdict confirms it: "by agents" goes', async () => {
    await openCreate(built('take'), theOwner, undefined, { key: 'one-way', hash: '#part-d/one-way', name: 'Which way do changes go?' });
    await waitFor(() => expect(button(/^Take$/)).not.toBeNull());
    expect(stateLine()).toBe('Decided by agents.');
    fireEvent.click(button(/^Take$/) as HTMLElement);
    confirm(/^Confirm take$/);
    await waitFor(() => expect(stateLine()).toBe('Decided.'));
    expect(button(/^Drop$/)).toBeNull();
  });

  it('a refusal shows the plan tool\'s message and leaves the entry as it was', async () => {
    const message = 'create needs calls, not taken yet';
    await openCreate(built(), theOwner, () => ({ status: 409, body: { error: 'refused', message } }));
    await waitFor(() => expect(button(/^Take$/)).not.toBeNull());
    fireEvent.click(button(/^Take$/) as HTMLElement);
    confirm(/^Confirm take$/);
    await waitFor(() => expect(within(item()).getByRole('alert')).toHaveTextContent(message));
    expect(stateLine()).toBe('Not decided.');
    expect(button(/^Take$/)).toBeEnabled();
    // The next press clears it.
    fireEvent.click(button(/^Drop$/) as HTMLElement);
    confirm(/^Confirm drop$/);
    await waitFor(() => expect(within(item()).getByRole('alert')).toHaveTextContent(message));
  });

  it('asks once more when the answer is lost (no connection, none in time, or the connection cut while the answer is read): the verdict may be on main, and the server then says so', async () => {
    const lost = [() => Promise.reject(new TypeError('Failed to fetch')), () => new Promise<never>(() => {}), () => ({ ...ok({ ok: true, commit: 'e'.repeat(40) }), cut: true })] as const;
    for (const first of lost) {
      const saved = apiConfig.writeTimeoutMs;
      apiConfig.writeTimeoutMs = 50;
      try {
        let asked = 0;
        const calls = await openCreate(built(), theOwner, () => {
          asked += 1;
          return asked === 1 ? first() : ok({ ok: true, commit: null, already: true });
        });
        await waitFor(() => expect(button(/^Take$/)).not.toBeNull());
        fireEvent.click(button(/^Take$/) as HTMLElement);
        confirm(/^Confirm take$/);
        await waitFor(() => expect(stateLine()).toBe('Decided.'));
        expect(posts(calls, 'verdict')).toHaveLength(2);
        expect(posts(calls, 'verdict')[1].body).toEqual(posts(calls, 'verdict')[0].body);
        expect(within(item()).queryByRole('alert')).toBeNull();
      } finally {
        apiConfig.writeTimeoutMs = saved;
        cleanup();
        localStorage.clear();
      }
    }
    // Lost twice: the failure shows, and the entry is left as it was.
    const calls = await openCreate(built(), theOwner, () => Promise.reject(new TypeError('Failed to fetch')));
    await waitFor(() => expect(button(/^Take$/)).not.toBeNull());
    fireEvent.click(button(/^Take$/) as HTMLElement);
    confirm(/^Confirm take$/);
    await waitFor(() => expect(within(item()).getByRole('alert')).toHaveTextContent('No connection.'));
    expect(posts(calls, 'verdict')).toHaveLength(2);
    expect(stateLine()).toBe('Not decided.');
  });

  it('a session gone signs the page out and writes nothing', async () => {
    await openCreate(built(), theOwner, () => refused(401, 'Sign in again.'));
    await waitFor(() => expect(button(/^Take$/)).not.toBeNull());
    fireEvent.click(button(/^Take$/) as HTMLElement);
    confirm(/^Confirm take$/);
    await waitFor(() => expect(button(/^Take$/)).toBeNull());
    expect(stateLine()).toBe('Not decided.');
  });
});

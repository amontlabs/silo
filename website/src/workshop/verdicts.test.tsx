// The owner's Take and Drop on an open entry: who sees them, the press that arms and the press that sends, the
// pending state, the verdict shown at once and kept across a reload until a build carries it, and a refusal.
import { act, cleanup, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { Me } from './api';
import { ARM_MS } from './Entry';
import { buildData, DECISIONS } from './fixtures';
import type { BuildData } from './plan';
import { mockApi, ok, owner, posts, refused, renderApp, rowButton, visitor, withheldTimers } from './test-support';
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
    fireEvent.click(button(/^Confirm drop$/) as HTMLElement);
    await waitFor(() => expect(posts(calls, 'verdict')).toHaveLength(1));
    expect(posts(calls, 'verdict')[0].body).toEqual({ entry: 'create', verdict: 'drop', built: BUILT });
    expect(posts(calls, 'verdict')[0].headers).toMatchObject({ 'X-Workshop': '1' });
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

  it('is pending while the server writes, then shows the verdict at once, kept across a reload until another build', async () => {
    let answer: (reply: Reply) => void = () => {};
    const data = built();
    await openCreate(data, theOwner, () => new Promise((resolve) => { answer = resolve; }));
    await waitFor(() => expect(button(/^Take$/)).not.toBeNull());
    expect(stateLine()).toBe('Not decided.');
    fireEvent.click(button(/^Take$/) as HTMLElement);
    fireEvent.click(button(/^Confirm take$/) as HTMLElement);
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
    // Another build carries the plan as it is then: the page shows that build's plan alone.
    await openCreate({ ...data, builtFrom: 'd'.repeat(40) }, theOwner);
    expect(stateLine()).toBe('Not decided.');
  });

  it('on an entry the agents decided, the owner\'s verdict confirms it: "by agents" goes', async () => {
    await openCreate(built('take'), theOwner, undefined, { key: 'one-way', hash: '#part-d/one-way', name: 'Which way do changes go?' });
    await waitFor(() => expect(button(/^Take$/)).not.toBeNull());
    expect(stateLine()).toBe('Decided by agents.');
    fireEvent.click(button(/^Take$/) as HTMLElement);
    fireEvent.click(button(/^Confirm take$/) as HTMLElement);
    await waitFor(() => expect(stateLine()).toBe('Decided.'));
    expect(button(/^Drop$/)).toBeNull();
  });

  it('a refusal shows the plan tool\'s message and leaves the entry as it was', async () => {
    const message = 'create needs calls, not taken yet';
    await openCreate(built(), theOwner, () => ({ status: 409, body: { error: 'refused', message } }));
    await waitFor(() => expect(button(/^Take$/)).not.toBeNull());
    fireEvent.click(button(/^Take$/) as HTMLElement);
    fireEvent.click(button(/^Confirm take$/) as HTMLElement);
    await waitFor(() => expect(within(item()).getByRole('alert')).toHaveTextContent(message));
    expect(stateLine()).toBe('Not decided.');
    expect(button(/^Take$/)).toBeEnabled();
    // The next press clears it.
    fireEvent.click(button(/^Drop$/) as HTMLElement);
    fireEvent.click(button(/^Confirm drop$/) as HTMLElement);
    await waitFor(() => expect(within(item()).getByRole('alert')).toHaveTextContent(message));
  });

  it('a session gone signs the page out and writes nothing', async () => {
    await openCreate(built(), theOwner, () => refused(401, 'Sign in again.'));
    await waitFor(() => expect(button(/^Take$/)).not.toBeNull());
    fireEvent.click(button(/^Take$/) as HTMLElement);
    fireEvent.click(button(/^Confirm take$/) as HTMLElement);
    await waitFor(() => expect(button(/^Take$/)).toBeNull());
    expect(stateLine()).toBe('Not decided.');
  });
});

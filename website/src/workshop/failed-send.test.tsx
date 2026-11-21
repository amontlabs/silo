// A comment or a suggestion whose sending fails never discards what the person typed: its text goes back as it was
// typed into an empty box, and above what was typed meanwhile, with a blank line between.
import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { App } from './App';
import { buildData } from './fixtures';
import { mockApi, owner, refused } from './test-support';
import type { Reply } from './test-support';
import { putBack } from './useWorkshop';

/** A route that answers when the test says so. */
function held() {
  let answer: (reply: Awaited<Reply>) => void = () => {};
  return { route: () => new Promise<Awaited<Reply>>((resolve) => { answer = resolve; }), fail: () => act(async () => answer(refused(502, 'GitHub did not answer.'))) };
}

async function openCore(post: ReturnType<typeof held>) {
  mockApi({ me: owner, routes: { 'POST comment': post.route } });
  history.replaceState(null, '', '/workshop#part-a/core');
  render(<App data={buildData()} />);
  const panel = await screen.findByRole('textbox', { name: 'Comment' }).then(() => document.getElementById('entry-core-more') as HTMLElement);
  return { box: within(panel).getByRole('textbox', { name: 'Comment' }), send: () => userEvent.click(within(panel).getByRole('button', { name: 'Comment' })) };
}

async function openSuggest(post: ReturnType<typeof held>) {
  mockApi({ me: owner, routes: { 'POST suggest': post.route } });
  history.replaceState(null, '', '/workshop#suggested');
  render(<App data={buildData()} />);
  const title = await screen.findByRole('textbox', { name: 'Title' });
  return { title, why: screen.getByRole('textbox', { name: 'Why' }), send: () => userEvent.click(screen.getByRole('button', { name: 'Suggest' })) };
}

describe('a comment whose sending fails', () => {
  it('goes back into its empty box as it was typed, spaces and lines included', async () => {
    const post = held();
    const { box, send } = await openCore(post);
    await userEvent.type(box, '  First line{Shift>}{Enter}{/Shift}second.  ');
    await send();
    expect(box).toHaveValue('');
    await post.fail();
    expect(box).toHaveValue('  First line\nsecond.  ');
  });

  it('goes above what was typed meanwhile, with a blank line between', async () => {
    const post = held();
    const { box, send } = await openCore(post);
    await userEvent.type(box, 'Sent first.');
    await send();
    await userEvent.type(box, 'Typed after.');
    await post.fail();
    expect(box).toHaveValue('Sent first.\n\nTyped after.');
  });
});

describe('a suggestion whose sending fails', () => {
  it('goes back into its empty form as it was typed', async () => {
    const post = held();
    const { title, why, send } = await openSuggest(post);
    await userEvent.type(title, ' Pause ');
    await userEvent.type(why, 'To come back.  ');
    await send();
    expect(title).toHaveValue('');
    await post.fail();
    expect(title).toHaveValue(' Pause ');
    expect(why).toHaveValue('To come back.  ');
  });

  it('keeps what was typed meanwhile, and puts its own title and text above the new text', async () => {
    const post = held();
    const { title, why, send } = await openSuggest(post);
    await userEvent.type(title, 'Pause');
    await userEvent.type(why, 'To come back.');
    await send();
    await userEvent.type(title, 'Resume');
    await userEvent.type(why, 'And go on.');
    await post.fail();
    expect(title).toHaveValue('Resume');
    expect(why).toHaveValue('Pause\n\nTo come back.\n\nAnd go on.');
  });

  it('keeps what was typed meanwhile in the text alone, its own text above it', async () => {
    const post = held();
    const { title, why, send } = await openSuggest(post);
    await userEvent.type(title, 'Pause');
    await userEvent.type(why, 'To come back.');
    await send();
    await userEvent.type(why, 'And go on.');
    await post.fail();
    expect(title).toHaveValue('Pause');
    expect(why).toHaveValue('To come back.\n\nAnd go on.');
  });
});

describe('putting back a failed text', () => {
  // Random texts, the failed one and the one typed meanwhile: neither is ever lost, the failed one comes first.
  it('keeps both texts whole, the failed one first', () => {
    let seed = 11;
    const random = (n: number) => {
      seed = (seed * 1103515245 + 12345) % 2 ** 31;
      return seed % n;
    };
    const pieces = ['', ' ', 'a', 'Two words', '\n', 'line\nbreak', '  padded  '];
    for (let run = 0; run < 500; run += 1) {
      const failed = pieces[random(pieces.length)] + pieces[random(pieces.length)];
      const now = pieces[random(pieces.length)] + pieces[random(pieces.length)];
      const back = putBack(failed, now);
      expect(back.startsWith(failed)).toBe(true);
      expect(back.endsWith(now)).toBe(true);
      expect(back).toBe(now === '' ? failed : failed === '' ? now : `${failed}\n\n${now}`);
    }
  });
});

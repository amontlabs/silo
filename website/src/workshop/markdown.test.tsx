// The notes of an entry, rendered as Markdown.
import userEvent from '@testing-library/user-event';
import { within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { buildData } from './fixtures';
import { notesButton, openCore, shownNotes } from './test-support';

describe('notes', () => {
  it('show headings, paragraphs, lists, nested and ordered lists, code and http links once opened', async () => {
    await openCore();
    await userEvent.click(notesButton());
    expect(notesButton()).toHaveAttribute('aria-expanded', 'true');
    const md = await shownNotes();
    // The first "## Notes" heading is dropped: the toggle already says it.
    expect(within(md).queryByRole('heading', { name: 'Notes' })).not.toBeInTheDocument();
    expect(within(md).getByRole('heading', { level: 4, name: 'From the old code' })).toBeInTheDocument();
    expect(within(md).getByText(/^The engine owns every computer\./)).toBeInTheDocument();
    const link = within(md).getByRole('link', { name: 'the design' });
    expect(link).toHaveAttribute('href', 'https://example.com/design');
    expect(link.getAttribute('rel')).toContain('noopener');
    // A relative link keeps its label only.
    expect(within(md).queryByRole('link', { name: 'a file' })).not.toBeInTheDocument();
    expect(md.textContent).toContain('a file');
    expect(md.innerHTML).not.toContain('docs/x.md');
    const lists = md.querySelectorAll(':scope > ul, :scope > ol');
    expect([...lists].map((list) => list.tagName)).toEqual(['UL', 'OL']);
    expect([...lists[0].querySelectorAll(':scope > li')].map((li) => li.firstChild?.textContent)).toEqual(['It must restart cleanly.', 'Calls are ']);
    expect(lists[0].querySelector('li > ul > li')).toHaveTextContent('one per verb');
    expect([...lists[1].querySelectorAll('li')].map((li) => li.textContent)).toEqual(['First', 'Second']);
    expect(lists[0].querySelector('code')).toHaveTextContent('engine.call');
    expect(md.querySelector('pre > code')?.textContent).toBe('silo engine start');
    await userEvent.click(notesButton());
    expect(document.querySelector('.ws-md')).toBeNull();
  });
});

describe('links with brackets in their address', () => {
  it('keep one level of brackets inside an http or https address, in notes and in comments', async () => {
    const notes = 'See [Fork](https://en.wikipedia.org/wiki/Fork_(software)) and [b](https://x.test/a_(b)_c), then (aside).';
    await openCore(buildData({ notes: { core: notes } }));
    await userEvent.click(notesButton());
    const md = await shownNotes();
    expect(within(md).getByRole('link', { name: 'Fork' })).toHaveAttribute('href', 'https://en.wikipedia.org/wiki/Fork_(software)');
    expect(within(md).getByRole('link', { name: 'b' })).toHaveAttribute('href', 'https://x.test/a_(b)_c');
    expect(md.textContent).toBe('See Fork and b, then (aside).');
  });
});

// A part's place draws All's rows: its features with their steps, pips and meters, the decisions each needs folded
// under it, then the part's decisions that none of its features needs. Product's decisions are All's rows too.
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { Boundary } from './Boundary';
import { buildData, DECISIONS, FEATURES } from './fixtures';
import { mockApi, renderApp, rowButton } from './test-support';

const names = (root: ParentNode = document.querySelector('.ws-detail') as ParentNode) => [...root.querySelectorAll('.wa-name > span:first-child')].map((name) => name.textContent);
const headings = () => [...document.querySelectorAll('.ws-detail h3.wa-grp')].map((heading) => heading.textContent);
const groupNames = (title: string) => names(screen.getByRole('region', { name: title }));
const items = () => [...document.querySelectorAll('.ws-detail .wa-item')].map((item) => item.id);

/** Part D with a feature that needs a decision of its own part, a feature of Part A and a feature before it. */
const withNeeds = () => buildData({
  features: [...FEATURES, { id: 'fork-keeps', stage: 'fourth', part: 'part-d', name: 'Keep a fork apart', what: 'Nothing shared.', needs: ['one-way', 'core', 'fork'] }],
});

/** Part B with decisions and no feature, Part A with features and no decision. */
const oneKind = () => buildData({
  features: FEATURES.filter((feature) => feature.part !== 'part-b'),
  decisions: [...DECISIONS.filter((decision) => decision.part !== 'part-a'), { id: 'b-asks', stage: 'second', part: 'part-b', asks: 'Which way does it go?' }],
});

describe('a part\'s place', () => {
  afterEach(() => { Reflect.deleteProperty(HTMLElement.prototype, 'scrollIntoView'); });

  it('lists its features with their steps, then the decisions none of them needs; a decision a feature needs folds under it', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#part-d');
    await renderApp(withNeeds());
    expect(headings()).toEqual(['Features', 'Decisions']);
    expect(groupNames('Features')).toEqual(['How research enters the plan', 'One silo command', 'Fork a computer', 'Save a checkpoint', 'See what a fork changed', 'Install your agent', 'Keep a fork apart', 'Secrets you cannot read']);
    // "Which way do changes go?" is folded under the feature that needs it.
    expect(groupNames('Decisions')).toEqual(['Can a log be changed after the fact?']);
    const feature = rowButton('Keep a fork apart').closest('.wa-item') as HTMLElement;
    // The step is the feature's place among every feature, as All numbers it; the part column is empty here.
    expect(feature.querySelector('.wa-n')).toHaveTextContent('11');
    expect(feature.querySelector('.wa-part')).toHaveTextContent('');
    expect(within(feature).getByRole('img', { name: '0 of 1 decisions decided' })).toBeInTheDocument();
    await userEvent.click(within(feature).getByRole('button', { name: /^Its decisions/ }));
    expect(within(feature).getByRole('button', { name: /^Which way do changes go\?/ })).toBeInTheDocument();
    expect(names(feature)).toEqual(['Keep a fork apart', 'Which way do changes go?']);
  });

  it('leaves the counts and the dots of the part as they were', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#part-d');
    await renderApp();
    expect(document.querySelectorAll('.ws-detail .ws-pips i')).toHaveLength(7);
    expect([...document.querySelectorAll('.ws-detail .ws-tally')].map((tally) => tally.textContent?.replace(/\u00a0/g, ' '))).toEqual(['7 not decided', 'Decisions: 2 not decided']);
  });

  it('opens a decision named by the address under the feature that needs it, unfolded, and scrolls to it alone', async () => {
    const scrolled: string[] = [];
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value(this: HTMLElement) { scrolled.push(this.id); } });
    mockApi();
    history.replaceState(null, '', '/workshop#part-d/one-way');
    await renderApp(withNeeds());
    await waitFor(() => expect(rowButton('Which way do changes go')).toHaveAttribute('aria-expanded', 'true'));
    await waitFor(() => expect(scrolled).toEqual(['entry-one-way@entry-fork-keeps']));
    expect(location.hash).toBe('#part-d/one-way');
  });

  it('opens a row named by the address where it now stands, and opening or closing a row moves none', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#part-a/core-in-zig');
    await renderApp();
    await waitFor(() => expect(rowButton('Which language is the core written in')).toHaveAttribute('aria-expanded', 'true'));
    expect(within(screen.getByRole('region', { name: 'Decisions' })).getByRole('button', { name: /^Which language/ })).toBeInTheDocument();
    const before = items();
    expect(before).toEqual(['entry-core', 'entry-calls', 'entry-core-in-zig']);
    await userEvent.click(rowButton('The engine runs on its own'));
    expect(rowButton('The engine runs on its own')).toHaveAttribute('aria-expanded', 'true');
    expect(items()).toEqual(before);
    await userEvent.click(rowButton('The engine runs on its own'));
    await userEvent.click(rowButton('Which language is the core written in'));
    expect(items()).toEqual(before);
  });
});

describe('a part with one kind of entry', () => {
  it('shows no heading for the kind it has not', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#part-b');
    await renderApp(oneKind());
    expect(headings()).toEqual(['Decisions']);
    expect(screen.queryByRole('region', { name: 'Features' })).toBeNull();
    expect(names()).toEqual(['Which way does it go?']);
  });

  it('shows only "Features" for a part with no decision', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#part-a');
    await renderApp(oneKind());
    expect(headings()).toEqual(['Features']);
    expect(screen.queryByRole('region', { name: 'Decisions' })).toBeNull();
  });
});

describe('the other lists', () => {
  it('gives Product, which holds decisions only, no heading, each on its own line', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#product');
    await renderApp();
    expect(headings()).toEqual([]);
    expect(names()).toHaveLength(3);
    expect(document.querySelectorAll('.ws-detail .wa-item.d.flat')).toHaveLength(3);
  });

  it('gives the plain list shown when the page fails the same two groups, and Product none', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    const Broken = (): never => { throw new Error('render failed'); };
    render(<Boundary data={buildData()}><Broken /></Boundary>);
    const partA = screen.getByRole('region', { name: 'Part A' });
    expect([...partA.querySelectorAll('h3')].map((heading) => heading.textContent)).toEqual(['Features', 'Decisions']);
    expect([...partA.querySelectorAll('li')].map((item) => item.textContent?.split(' (')[0])).toEqual(['The engine runs on its own', 'Every call defined once', 'Which language is the core written in?']);
    expect(screen.getByRole('region', { name: 'Part B' }).querySelectorAll('h3')).toHaveLength(1);
    expect(screen.getByRole('region', { name: 'Product' }).querySelector('h3')).toBeNull();
  });
});

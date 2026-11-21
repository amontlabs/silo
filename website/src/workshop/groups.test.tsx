// A list that holds features and decisions shows the features first, under their heading.
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { Boundary } from './Boundary';
import { buildData, DECISIONS, FEATURES } from './fixtures';
import { mockApi, renderApp, rowButton } from './test-support';

const names = (root: ParentNode = document) => [...root.querySelectorAll('.ws-detail .ws-name')].map((name) => name.textContent);
const headings = () => [...document.querySelectorAll('.ws-detail h3.ws-grp')].map((heading) => heading.textContent);
const groupNames = (title: string) => names(screen.getByRole('region', { name: title }));

/** Part B with decisions and no feature, Part A with features and no decision. */
const oneKind = () => buildData({
  features: FEATURES.filter((feature) => feature.part !== 'part-b'),
  decisions: [...DECISIONS.filter((decision) => decision.part !== 'part-a'), { id: 'b-asks', stage: 'second', part: 'part-b', asks: 'Which way does it go?' }],
});

describe('a part with features and decisions', () => {
  afterEach(() => { Reflect.deleteProperty(HTMLElement.prototype, 'scrollIntoView'); });

  it('lists its features under "Features", then its decisions under "Decisions", each in the order of order.json', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#part-d');
    await renderApp();
    expect(headings()).toEqual(['Features', 'Decisions']);
    expect(groupNames('Features')).toEqual(['How research enters the plan', 'One silo command', 'Fork a computer', 'Save a checkpoint', 'See what a fork changed', 'Install your agent', 'Secrets you cannot read']);
    expect(groupNames('Decisions')).toEqual(['Which way do changes go?', 'Can a log be changed after the fact?']);
    // The features come first, the decisions after them.
    expect(names()).toEqual([...groupNames('Features'), ...groupNames('Decisions')]);
  });

  it('leaves the counts and the dots of the part as they were', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#part-d');
    await renderApp();
    expect(document.querySelectorAll('.ws-detail .ws-pips i')).toHaveLength(7);
    expect([...document.querySelectorAll('.ws-detail .ws-tally')].map((tally) => tally.textContent?.replace(/ /g, ' '))).toEqual(['7 not decided', 'Decisions: 2 not decided']);
  });

  it('opens a row named by the address where it now stands and scrolls to that row alone, and opening or closing a row moves none', async () => {
    const scrolled: string[] = [];
    // jsdom has no scrollIntoView; the page calls it on the row it brings into view.
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value(this: HTMLElement) { scrolled.push(this.id); } });
    mockApi();
    history.replaceState(null, '', '/workshop#part-a/core-in-zig');
    await renderApp();
    await waitFor(() => expect(rowButton('Which language is the core written in')).toHaveAttribute('aria-expanded', 'true'));
    await waitFor(() => expect(scrolled).toEqual(['entry-core-in-zig']));
    expect(within(screen.getByRole('region', { name: 'Decisions' })).getByRole('button', { name: /^Which language/ })).toBeInTheDocument();
    const items = () => [...document.querySelectorAll('.ws-detail .ws-item')].map((item) => item.id);
    // The row opened from the address and the two features above it, in their places.
    const before = items();
    expect(before).toEqual(['entry-core', 'entry-calls', 'entry-core-in-zig']);
    expect(names()).toEqual(['The engine runs on its own', 'Every call defined once', 'Which language is the core written in?']);
    await userEvent.click(rowButton('The engine runs on its own'));
    expect(rowButton('The engine runs on its own')).toHaveAttribute('aria-expanded', 'true');
    expect(items()).toEqual(before);
    await userEvent.click(rowButton('The engine runs on its own'));
    await userEvent.click(rowButton('Which language is the core written in'));
    expect(items()).toEqual(before);
    expect(names()).toEqual(['The engine runs on its own', 'Every call defined once', 'Which language is the core written in?']);
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

describe('the other lists that hold both kinds', () => {
  it('lists what is left out as features, then decisions', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#left');
    await renderApp();
    expect(headings()).toEqual(['Features', 'Decisions']);
    expect(groupNames('Features')).toEqual(['A hosted reviewer', 'An earlier desktop stack']);
    expect(groupNames('Decisions')).toEqual(['Which format does an export use?']);
  });

  it('gives Product, which holds decisions only, no heading', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#product');
    await renderApp();
    expect(headings()).toEqual([]);
    expect(names()).toHaveLength(3);
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

import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { fillHomePlan } from './home-plan.ts';

const html = readFileSync(join(process.cwd(), 'index.html'), 'utf8');
const css = readFileSync(join(process.cwd(), 'src/style.css'), 'utf8');
const loopScript = readFileSync(join(process.cwd(), 'src/loop.ts'), 'utf8');
const theme = readFileSync(join(process.cwd(), 'public/theme.js'), 'utf8');
const workshop = readFileSync(join(process.cwd(), 'workshop/index.html'), 'utf8');
const PITCH = 'Your agent gets its own computer and forks a copy for each task. You see everything the copy changed, then merge it or drop it.';

const parse = (source: string) => new DOMParser().parseFromString(source, 'text/html');
const doc = parse(html);
const text = (selector: string, from: Document = doc) =>
  [...from.querySelectorAll(selector)].map((node) => node.textContent?.replace(/\s+/g, ' ').trim());

const STAGES = JSON.stringify([
  { id: 'first', title: 'First', line: 'A fixture stage with a line.' },
  { id: 'second', title: 'Second', line: 'Never shown.' },
  { id: 'gone', title: 'Gone', line: 'Old things.' },
]);
const feature = (n: number, stage: string, verdict: string | null) =>
  JSON.stringify({ n, id: `f${n}`, name: `Feature ${n}`, stage, what: 'Something.', verdict });
const FEATURES = [
  feature(1, 'first', 'take'),
  feature(2, 'first', 'take'),
  feature(3, 'second', 'refine'),
  feature(4, 'second', null),
  feature(5, 'gone', 'take'),
  feature(6, 'gone', 'drop'),
].join('\n');

const repositories: string[] = [];
function fixtureRepository(files: { stages?: string; features?: string }): string {
  const root = mkdtempSync(join(tmpdir(), 'silo-home-plan-'));
  repositories.push(root);
  mkdirSync(join(root, 'docs/product'), { recursive: true });
  if (files.stages !== undefined) writeFileSync(join(root, 'docs/product/stages.json'), files.stages);
  if (files.features !== undefined) writeFileSync(join(root, 'docs/product/features.jsonl'), files.features);
  return root;
}
afterEach(() => {
  for (const root of repositories.splice(0)) rmSync(root, { recursive: true, force: true });
});

describe('the home page', () => {
  it('leads with the headline and the pitch', () => {
    expect(text('h1')).toEqual(['Computers your agents can fork.']);
    expect(text('.pitch')).toEqual([PITCH]);
    expect(doc.querySelector('meta[name="description"]')?.getAttribute('content')).toBe(PITCH);
    expect(doc.querySelector('meta[property="og:description"]')?.getAttribute('content')).toBe(PITCH);
  });

  it('offers the build and the source, and nothing of the old app', () => {
    expect(html).not.toMatch(/download|releases|demo\.html|silo-tour|How to use|\/media\//i);
    const actions = [...doc.querySelectorAll('.hero-actions a')];
    expect(actions.map((link) => link.textContent?.trim())).toEqual(['Follow the build', 'View source']);
    expect(actions.map((link) => link.getAttribute('href'))).toEqual(['/workshop', 'https://github.com/amontlabs/silo']);
    expect(doc.querySelector('.door a')?.getAttribute('href')).toBe('/workshop');
  });

  it('has the navigation, with GitHub kept on phones', () => {
    const links = [...doc.querySelectorAll('.site-header nav a')];
    expect(links.map((link) => link.textContent?.trim())).toEqual(['Workshop', 'GitHub']);
    expect(links.map((link) => link.getAttribute('href'))).toEqual(['/workshop', 'https://github.com/amontlabs/silo']);
    expect(links.some((link) => link.classList.contains('source-nav'))).toBe(false);
    expect(text('.site-footer nav a')).toEqual(['Workshop', 'GitHub', 'Report an issue']);
  });

  it('draws arrows and checks as icons, not as characters the bundled fonts lack', () => {
    expect(html).not.toMatch(/[↗↑→✓]/);
  });

  it('shows the loop in reading order without script', () => {
    expect(text('.loop .pc-what, .loop .end')).toEqual([
      'Your agent and your setup',
      'Upgrade Postgres',
      'merge',
      'Try a new parser',
      'drop',
    ]);
    expect(text('.points p')[2]).toBe('Silo runs on machines you own, starting with your Mac. Nothing is hosted.');
    expect(text('.loop figcaption')).toEqual(['Main is forked into two copies: one is merged back into main, the other is dropped.']);
  });

  it('fills the two plan lines from the plan files', () => {
    const filled = parse(fillHomePlan(html, fixtureRepository({ stages: STAGES, features: FEATURES })));
    expect(text('.hero-note', filled)).toEqual(['Not out yet. First up: a fixture stage with a line.']);
    expect(text('.door-line', filled)).toEqual(['2 of 4 features are planned. Have a say in the rest.']);
    expect(text('.door-line span', filled)).toEqual(['2 of 4 features are planned.', 'Have a say in the rest.']);
  });

  it('writes a plan line with $ or markup in it as text', () => {
    const line = 'Costs $& and $` and $\' and $$ <b>bold</b>.';
    const stages = JSON.stringify([{ id: 'first', title: 'First', line }]);
    const filled = fillHomePlan(html, fixtureRepository({ stages, features: feature(1, 'first', 'take') }));
    const page = parse(filled);
    expect(text('.hero-note', page)).toEqual(['Not out yet. First up: costs $& and $` and $\' and $$ <b>bold</b>.']);
    expect(page.querySelector('.hero-note b')).toBeNull();
    expect(filled.length).toBeLessThan(html.length + 200);
  });

  it('stops the build when a plan file is missing or malformed', () => {
    expect(() => fillHomePlan(html, fixtureRepository({ stages: STAGES }))).toThrow(/features\.jsonl: cannot be read/);
    expect(() => fillHomePlan(html, fixtureRepository({ features: FEATURES }))).toThrow(/stages\.json: cannot be read/);
    expect(() => fillHomePlan(html, fixtureRepository({ stages: '[', features: FEATURES }))).toThrow(/stages\.json: not valid JSON/);
    expect(() => fillHomePlan(html, fixtureRepository({ stages: STAGES, features: '{"n": 1}' }))).toThrow(/features\.jsonl:1/);
    const lineless = JSON.stringify([{ id: 'first', title: 'First' }]);
    expect(() => fillHomePlan(html, fixtureRepository({ stages: lineless, features: '' }))).toThrow(/first stage has no line/);
  });

  it('keeps no style for a class or id the page and its script do not use', () => {
    const used = new Set([
      ...[...html.matchAll(/class="([^"]*)"/g)].flatMap((match) => match[1].split(/\s+/)),
      ...[...loopScript.matchAll(/['"`.]([a-z][\w-]*)/g)].map((match) => match[1]),
      ...[...loopScript.matchAll(/class="([^"$]*)/g)].flatMap((match) => match[1].split(/\s+/)),
    ]);
    const ids = new Set([...html.matchAll(/id="([^"]*)"/g)].map((match) => match[1]));
    const selectors = css.replace(/\/\*[\s\S]*?\*\//g, '').match(/[^{}]+(?=\{)/g) ?? [];
    const unused = selectors
      .filter((selector) => !selector.trim().startsWith('@'))
      .flatMap((selector) => [...selector.matchAll(/([.#])([A-Za-z][\w-]*)/g)])
      .filter(([, kind, name]) => (kind === '.' ? !used.has(name) : !ids.has(name)))
      .map(([whole]) => whole);
    expect([...new Set(unused)]).toEqual([]);
  });

  it('has a theme script with only what the two pages use', () => {
    expect(theme).not.toMatch(/iframe|postMessage|data-dark-source/);
    expect(html).toContain('data-theme-picker');
    expect(workshop).toContain('data-theme-picker');
  });
});

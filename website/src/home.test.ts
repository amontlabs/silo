import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { canonicalText } from '../../tools/plan/model/index.js';
import { planFiles } from './workshop/fixtures.ts';
import { fillHomePlan, homePlanLines } from './home-plan.ts';
import { readBuildData } from './workshop/data.ts';
import { buildPlan } from './workshop/plan.ts';

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

// A repository folder holding the fixture plan, written as plan/ files.
const repositories: string[] = [];
type Files = Map<string, string>;
function fixtureRepository(edit?: (files: Files) => void): string {
  const root = mkdtempSync(join(tmpdir(), 'silo-home-plan-'));
  repositories.push(root);
  const files = planFiles();
  edit?.(files);
  for (const [name, content] of files) {
    mkdirSync(dirname(join(root, 'plan', name)), { recursive: true });
    writeFileSync(join(root, 'plan', name), content);
  }
  return root;
}
const fill = (edit?: (files: Files) => void) => fillHomePlan(html, fixtureRepository(edit));
/** The fixture plan with the first stage's line replaced. */
const firstLine = (line: string) => (files: Files) => {
  const order = JSON.parse(files.get('order.json') as string) as { stages: { line: string }[] };
  order.stages[0].line = line;
  files.set('order.json', canonicalText(order));
};
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
    const filled = parse(fill());
    expect(text('.hero-note', filled)).toEqual(['Not out yet. First up: one computer you can open from the app and the command line.']);
    // Three taken entries of the 18 outside the stage `gone`.
    expect(text('.door-line', filled)).toEqual(['3 of 18 features are planned. Have a say in the rest.']);
    expect(text('.door-line span', filled)).toEqual(['3 of 18 features are planned.', 'Have a say in the rest.']);
    expect(homePlanLines(fixtureRepository()).door).toContain('<span>3 of 18 features are planned.</span>');
  });

  it('counts planned as the map does: a built feature is built, not planned, and a dropped one is not counted', () => {
    const root = fixtureRepository((files) => {
      const engine = JSON.parse(files.get('features/engine.json') as string);
      files.set('features/engine.json', canonicalText({ ...engine, built: '2026-11-01' }));
      const fork = JSON.parse(files.get('features/fork.json') as string);
      files.set('features/fork.json', canonicalText({ ...fork, verdict: 'drop' }));
    });
    const plan = buildPlan(readBuildData(root), null);
    const planned = plan.areas.reduce((sum, group) => sum + group.counts.planned, 0);
    const total = plan.areas.reduce((sum, group) => sum + group.rows.length - group.counts.dropped, 0);
    expect([planned, total]).toEqual([2, 17]);
    expect(homePlanLines(root).door).toContain('<span>2 of 17 features are planned.</span>');
  });

  it('writes a plan line with $ or markup in it as text', () => {
    const line = 'Costs $& and $` and $\' and $$ <b>bold</b>.';
    const filled = fill(firstLine(line));
    const page = parse(filled);
    expect(text('.hero-note', page)).toEqual(['Not out yet. First up: costs $& and $` and $\' and $$ <b>bold</b>.']);
    expect(page.querySelector('.hero-note b')).toBeNull();
    expect(filled.length).toBeLessThan(html.length + 200);
  });

  it('stops the build when the plan is missing or malformed', () => {
    expect(() => fillHomePlan(html, join(tmpdir(), 'silo-home-plan-missing'))).toThrow('plan/: cannot be read');
    expect(() => fill((files) => files.delete('order.json'))).toThrow(/order\.json/);
    expect(() => fill((files) => files.set('areas.json', '['))).toThrow(/areas\.json/);
    expect(() => fill((files) => files.set('features/engine.json', '{'))).toThrow(/features\/engine\.json/);
    expect(() => fill((files) => files.set('decisions.json', '{}'))).toThrow(/decisions\.json/);
    expect(() => fill(firstLine(''))).toThrow(/line/);
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

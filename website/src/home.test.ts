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

// Fixture plan files, valid for every reader of the plan: features.jsonl entries outside the stage
// `gone` carry an area, areas.json lists every area the Workshop's map draws, each with an entry.
const AREA_IDS = ['engine', 'computers', 'app', 'checkpoints', 'forks', 'changes', 'agents', 'access', 'setup', 'tasks', 'devices', 'releases', 'trust', 'desktop', 'macos', 'sharing'];
const AREAS = JSON.stringify(AREA_IDS.map((id) => ({ id, title: `Area ${id}`, line: `What ${id} covers.` })), null, 1);
const DECISIONS = '# Decisions\n\n- 2026-11-16 · owner · A fixture decision.\n';
const stages = (firstLine?: string) =>
  JSON.stringify([
    { id: 'first', title: 'First', ...(firstLine === undefined ? {} : { line: firstLine }) },
    { id: 'second', title: 'Second', line: 'Never shown.' },
    { id: 'gone', title: 'Gone', line: 'Old things.' },
  ]);
const STAGES = stages('A fixture stage with a line.');
const feature = (n: number, stage: string, verdict: string | null, area?: string) =>
  JSON.stringify({ n, id: `f${n}`, name: `Feature ${n}`, stage, what: 'Something.', verdict, ...(area ? { area } : {}) });
// 16 entries in areas (2 planned), and 2 in the stage `gone` (one planned, which does not count).
const FEATURES = [
  ...AREA_IDS.map((area, index) => feature(index + 1, index < 8 ? 'first' : 'second', index < 2 ? 'take' : index % 3 ? 'refine' : null, area)),
  feature(17, 'gone', 'take'),
  feature(18, 'gone', 'drop'),
].join('\n');

const repositories: string[] = [];
function fixtureRepository(files: { stages?: string; features?: string; areas?: string; decisions?: string }): string {
  const root = mkdtempSync(join(tmpdir(), 'silo-home-plan-'));
  repositories.push(root);
  mkdirSync(join(root, 'docs/product'), { recursive: true });
  const write = (name: string, content: string | undefined) => {
    if (content !== undefined) writeFileSync(join(root, 'docs/product', name), content);
  };
  write('stages.json', files.stages);
  write('features.jsonl', files.features);
  write('areas.json', files.areas ?? AREAS);
  write('DECISIONS.md', files.decisions ?? DECISIONS);
  return root;
}
const fill = (files: Parameters<typeof fixtureRepository>[0]) => {
  const root = fixtureRepository(files);
  return fillHomePlan(html, root, join(root, 'recordings'));
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
    const filled = parse(fill({ stages: STAGES, features: FEATURES }));
    expect(text('.hero-note', filled)).toEqual(['Not out yet. First up: a fixture stage with a line.']);
    expect(text('.door-line', filled)).toEqual(['2 of 16 features are planned. Have a say in the rest.']);
    expect(text('.door-line span', filled)).toEqual(['2 of 16 features are planned.', 'Have a say in the rest.']);
  });

  it('writes a plan line with $ or markup in it as text', () => {
    const line = 'Costs $& and $` and $\' and $$ <b>bold</b>.';
    const filled = fill({ stages: stages(line), features: FEATURES });
    const page = parse(filled);
    expect(text('.hero-note', page)).toEqual(['Not out yet. First up: costs $& and $` and $\' and $$ <b>bold</b>.']);
    expect(page.querySelector('.hero-note b')).toBeNull();
    expect(filled.length).toBeLessThan(html.length + 200);
  });

  it('stops the build when a plan file is missing or malformed', () => {
    expect(() => fill({ stages: STAGES })).toThrow(/features\.jsonl: cannot be read/);
    expect(() => fill({ features: FEATURES })).toThrow(/stages\.json: cannot be read/);
    expect(() => fill({ stages: '[', features: FEATURES })).toThrow(/stages\.json: not valid JSON/);
    expect(() => fill({ stages: STAGES, features: '{"n": 1}' })).toThrow(/features\.jsonl:1/);
    expect(() => fill({ stages: STAGES, features: FEATURES, decisions: '- not a decision line' })).toThrow(/DECISIONS\.md:1/);
    expect(() => fill({ stages: stages(), features: FEATURES })).toThrow(/first stage has no line/);
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

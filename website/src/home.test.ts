import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const html = readFileSync(join(process.cwd(), 'index.html'), 'utf8');
const read = (file: string) => readFileSync(join(process.cwd(), file), 'utf8');
const css = ['src/style.css', 'src/flow.css', 'src/cards.css'].map(read).join('\n');
const scripts = ['src/home.ts', 'src/pictures.ts', 'src/flow.ts', 'src/cards.ts', 'src/scene.ts'].map(read).join('\n');
const theme = readFileSync(join(process.cwd(), 'public/theme.js'), 'utf8');
const workshop = readFileSync(join(process.cwd(), 'workshop/index.html'), 'utf8');
const PITCH = 'Your agent gets its own computer and forks a copy for each task. You see everything the copy changed, then merge it or drop it.';

const parse = (source: string) => new DOMParser().parseFromString(source, 'text/html');
const doc = parse(html);
const text = (selector: string, from: Document = doc) =>
  [...from.querySelectorAll(selector)].map((node) => node.textContent?.replace(/\s+/g, ' ').trim());

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
    expect(doc.querySelector('main > :first-child')?.matches('a.strip[href="/workshop"]')).toBe(true);
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

  it('tells the flow in reading order without script, and in words for a screen reader', () => {
    expect(text('.flow .title')).toEqual(['Git for computers.']);
    // The teammate's computer and its fork carry the teammate's initial beside the name.
    expect(text('.stage .pc-name')).toEqual(['main', 'pg-17', 'stripe', 'searchM', 'Msearch']);
    expect(text('.stage .pc-what')).toEqual([
      'Your agent and your setup',
      'Upgrade Postgres',
      'Try Stripe checkout',
      'Rebuild search',
      'Rebuild search',
    ]);
    expect(text('.stage .fa .diff .dr > span:first-child')).toEqual(['code', 'packages', 'settings', 'database']);
    expect(text('.stage .fa .diff .d4 > span:last-child')).toEqual(['schema changed']);
    // The picture is drawn for the eye; a screen reader gets the caption instead.
    expect(doc.querySelector('.stage')?.getAttribute('aria-hidden')).toBe('true');
    const figure = doc.querySelector('.flow figure');
    expect(figure?.getAttribute('aria-describedby')).toBe('flow-caption');
    expect(text('#flow-caption')[0]).toMatch(/^Two tasks are given to the agent in main.*you fork its checkpoint onto your Mac to check the work\.$/);
  });

  it('gives the flow controls a screen reader can name, also where a step shows no name', () => {
    expect(text('.flow .ctl')).toEqual(['Pause']);
    const steps = [...doc.querySelectorAll('.steps li > button[type="button"]')];
    expect(steps.map((button) => button.getAttribute('aria-label'))).toEqual(['Fork', 'Work', 'Screen', 'Secrets', 'Diff', 'Merge or drop', 'Other devices']);
    expect(text('.steps .lb')).toEqual(steps.map((button) => button.getAttribute('aria-label')));
    // What is drawn inside the picture is not a control: nothing in it takes the keyboard.
    expect(doc.querySelectorAll('.stage :is(button, a, input, [tabindex])')).toHaveLength(0);
  });

  it('says who it is for, in nine cards: a picture, a label, what Silo does about it, one sentence', () => {
    expect(text('.for .title')).toEqual(['For high-velocity developers and small teams.']);
    expect(text('.points .term')).toEqual([
      'recipes',
      'your tools',
      'secrets',
      'network rules',
      'review',
      'merge requests',
      'step history',
      'other devices',
      'linux, macos',
    ]);
    expect(text('.points h3')).toEqual([
      'A recipe is built once',
      'Your browser, editor and terminal reach in',
      'Secrets and tokens stay outside the VM',
      'Open, limited or closed, per computer',
      'A diff of the whole machine',
      'Everything waiting to merge, in one list',
      'Go back, or fork from any step',
      'A computer from your server or a teammate',
      'Linux and macOS computers on one Mac',
    ]);
    expect(doc.querySelectorAll('.points > li.card > .txt > .term + h3 + p:last-child')).toHaveLength(9);
    expect(doc.querySelectorAll('.points > li.card > svg.mini[aria-hidden="true"][viewBox="0 0 320 140"]')).toHaveLength(9);
    expect(new Set([...doc.querySelectorAll('.card')].map((card) => card.getAttribute('data-mini'))).size).toBe(9);
  });

  it('says nothing the plan contradicts: no root, no row counts', () => {
    expect(doc.querySelector('main')?.textContent).not.toMatch(/\broot\b|\d+ rows/i);
  });

  it('takes every colour of the pictures from the theme, so the dark theme changes them', () => {
    const body = html.slice(html.indexOf('<main'));
    expect(body).not.toMatch(/(?:fill|stroke|style)="[^"]*#[0-9a-f]{3,8}/i);
    expect(scripts).not.toMatch(/#[0-9a-f]{3,8}\b/i);
  });

  it('says what the Workshop is, and where Silo stands, with nothing read from the plan', () => {
    expect(text('.strip-line')).toEqual(['Follow us building Silo live, get an overview of every feature, suggest your ideas and give feedback.']);
    expect(text('.notice p')).toEqual([
      'in heavy development',
      "Treat Silo as a build in public for now. If you try it, expect rough edges, breaking changes, no security guarantees and no data migration between versions. Don't keep anything in it that isn't backed up.",
    ]);
    expect(html).not.toMatch(/<!-- plan:|features are planned|First up|Not out yet/);
  });

  it('gives a shared link its picture: a 1200 x 630 PNG', () => {
    const content = (selector: string) => doc.querySelector(selector)?.getAttribute('content');
    expect(content('meta[property="og:image"]')).toBe('https://silo.amontlabs.com/og.png');
    expect(content('meta[name="twitter:card"]')).toBe('summary_large_image');
    const image = readFileSync(join(process.cwd(), 'public/og.png'));
    expect(image.subarray(1, 4).toString('latin1')).toBe('PNG');
    expect([image.readUInt32BE(16), image.readUInt32BE(20)]).toEqual([1200, 630]);
    expect([content('meta[property="og:image:width"]'), content('meta[property="og:image:height"]')]).toEqual(['1200', '630']);
  });

  it('waits only for fonts it preloads', () => {
    const waited = [...(html.match(/const faces = \[(.*?)\];/)?.[1].matchAll(/'((?:italic )?)\d+px "IBM Plex (\w+)"'/g) ?? [])].map(
      ([, italic, family]) => `/fonts/ibm-plex-${family.toLowerCase()}-400-${italic ? 'italic' : 'normal'}.woff2`,
    );
    const preloaded = [...doc.querySelectorAll('link[rel="preload"][as="font"]')].map((link) => link.getAttribute('href') as string);
    expect(waited).toHaveLength(4);
    expect([...preloaded].sort()).toEqual([...waited].sort());
    for (const href of preloaded) expect(existsSync(join(process.cwd(), 'public', href))).toBe(true);
  });

  it('keeps no style for a class or id the page and its scripts do not use', () => {
    const used = new Set([
      ...[...html.matchAll(/class="([^"]*)"/g)].flatMap((match) => match[1].split(/\s+/)),
      ...[...scripts.matchAll(/['"`.]([a-z][\w-]*)/g)].map((match) => match[1]),
      ...[...scripts.matchAll(/class="([^"$]*)/g)].flatMap((match) => match[1].split(/\s+/)),
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

  it('gives the pictures no class name the rest of the page already styles', () => {
    // A part of a picture named like the page's `go` or `out` icon would take that icon's size.
    const names = (source: string) =>
      new Set(
        (source.replace(/\/\*[\s\S]*?\*\//g, '').match(/[^{}]+(?=\{)/g) ?? [])
          .filter((selector) => !selector.trim().startsWith('@'))
          .flatMap((selector) => [...selector.matchAll(/\.([A-Za-z][\w-]*)/g)].map((match) => match[1])),
      );
    const page = new Set([
      ...names(read('src/site.css')),
      ...names(read('src/style.css')),
      ...names(html.slice(html.indexOf('<style>'), html.indexOf('</style>'))),
    ]);
    // What style.css styles on purpose: the pictures' own frames, hidden until they are drawn, and a scene being put at a moment.
    for (const own of ['stage', 'steps', 'ctl', 'mini', 'jump']) page.delete(own);
    const pictures = doc.querySelectorAll('.stage, .stage *, .steps, .steps *, .mini, .mini *');
    const inMarkup = [...pictures].flatMap((part) => [...part.classList]);
    // In the scripts: the class names in a selector, those a cue adds or removes, and those of the wires it draws.
    const code = read('src/flow.ts') + read('src/cards.ts') + read('src/scene.ts');
    const inScripts = [
      ...[...code.matchAll(/'[^'\n]*'|`[^`\n]*`/g)].flatMap(([literal]) => [...literal.matchAll(/\.([a-z][\w-]*)/g)].map((match) => match[1])),
      ...[...code.matchAll(/(?:add|rem|once|toggle|remove)\((?:'[^'\n]*', )?'([a-z][\w-]*)'/g)].map((match) => match[1]),
      ...[...code.matchAll(/class="([^"$]*)/g)].flatMap((match) => match[1].split(/\s+/)),
    ];
    expect([...new Set([...inMarkup, ...inScripts])].filter((name) => page.has(name))).toEqual([]);
  });

  it('has a theme script with only what the two pages use', () => {
    expect(theme).not.toMatch(/iframe|postMessage|data-dark-source/);
    expect(html).toContain('data-theme-picker');
    expect(workshop).toContain('data-theme-picker');
  });
});

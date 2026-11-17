import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

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

  it('shows the loop in reading order without script', () => {
    expect(text('.loop .pc-what, .loop .end')).toEqual([
      'Your agent and your setup',
      'Upgrade Postgres',
      'merge',
      'Try a new parser',
      'drop',
    ]);
    expect(text('.loop figcaption')).toEqual(['Main is forked into two copies: one is merged back into main, the other is dropped.']);
  });

  it('says who it is for, in six points: a term, what Silo does about it, one sentence', () => {
    expect(text('.for .title')).toEqual(['For high-velocity developers and small teams.']);
    expect(text('.points .term')).toEqual(['parallel agents', 'environment setup', 'full access', 'review', 'teams', 'where it runs']);
    expect(text('.points h3')).toEqual([
      'One virtual machine per agent',
      "Forks boot from main's disk",
      'Root in a VM with no route to your Mac',
      'A diff of the whole machine',
      'A shared host for the team',
      'Your Mac, a Linux server or the cloud',
    ]);
    expect(doc.querySelectorAll('.points li > h3 + p')).toHaveLength(6);
    expect(doc.querySelectorAll('.points .term svg[aria-hidden="true"]')).toHaveLength(6);
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

import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const html = readFileSync(join(process.cwd(), 'index.html'), 'utf8');
const read = (file: string) => readFileSync(join(process.cwd(), file), 'utf8');
const css = read('src/style.css');
const theme = readFileSync(join(process.cwd(), 'public/theme.js'), 'utf8');
const workshop = readFileSync(join(process.cwd(), 'workshop/index.html'), 'utf8');
const HEADLINE = 'Your agents work in a computer of their own.';
const PITCH = [
  'Silo records everything they do there, shows you what matters, and takes you back to any moment.',
  'The computer is a virtual machine on your Mac. Nothing of yours is inside unless you put it there.',
];

const parse = (source: string) => new DOMParser().parseFromString(source, 'text/html');
const doc = parse(html);
const text = (selector: string, from: Document = doc) =>
  [...from.querySelectorAll(selector)].map((node) => node.textContent?.replace(/\s+/g, ' ').trim());

describe('the home page', () => {
  it('leads with the headline and the pitch, and is titled Silo', () => {
    expect(text('h1')).toEqual([HEADLINE]);
    expect(text('.pitch span')).toEqual(PITCH);
    expect(doc.querySelector('meta[name="description"]')?.getAttribute('content')).toBe(PITCH[0]);
    expect(doc.querySelector('meta[property="og:description"]')?.getAttribute('content')).toBe(PITCH[0]);
    expect(doc.title).toBe('Silo');
    expect(doc.querySelector('meta[property="og:title"]')?.getAttribute('content')).toBe('Silo');
  });

  it('holds the way to the Workshop, then the hero, then the notice, and no script, picture or card', () => {
    expect([...doc.querySelectorAll('main > *')].map((node) => node.tagName + (node.className ? `.${node.className.split(' ')[0]}` : ''))).toEqual(['A.strip', 'SECTION.hero', 'DIV.notice']);
    expect([...doc.querySelectorAll('.hero > *')].map((node) => node.tagName + (node.id ? `#${node.id}` : ''))).toEqual(['H1#hero-title', 'P']);
    expect(doc.querySelectorAll('.hero a')).toHaveLength(0);
    expect(doc.querySelector('main > a.strip')?.getAttribute('href')).toBe('/workshop');
    expect(doc.querySelectorAll('script[type="module"], figure, .card, .stage')).toHaveLength(0);
  });

  it('describes the new design, not the loop of fork, diff and merge, and keeps nothing of the old app', () => {
    expect(html).not.toMatch(/download|releases|demo\.html|silo-tour|How to use|\/media\//i);
    expect(html).not.toMatch(/Git for computers|\bmerge|\bdiff\b|\bforks?\b/i);
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

  it('says nothing the plan contradicts: no root, no row counts', () => {
    expect(doc.querySelector('main')?.textContent).not.toMatch(/\broot\b|\d+ rows/i);
  });

  it('takes every colour of the page from the theme, so the dark theme changes them', () => {
    const body = html.slice(html.indexOf('<main'));
    expect(body).not.toMatch(/(?:fill|stroke|style)="[^"]*#[0-9a-f]{3,8}/i);
  });

  it('says what the Workshop is and where Silo stands, with nothing read from the plan', () => {
    expect(text('.strip-name')).toEqual(['Workshop']);
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
    expect(waited).toHaveLength(2);
    expect([...preloaded].sort()).toEqual([...waited].sort());
    for (const href of preloaded) expect(existsSync(join(process.cwd(), 'public', href))).toBe(true);
  });

  it('keeps no style for a class or id the page does not use', () => {
    const used = new Set([...html.matchAll(/class="([^"]*)"/g)].flatMap((match) => match[1].split(/\s+/)));
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

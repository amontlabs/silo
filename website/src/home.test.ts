import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const html = readFileSync(join(process.cwd(), 'index.html'), 'utf8');
const css = readFileSync(join(process.cwd(), 'src/style.css'), 'utf8');
const theme = readFileSync(join(process.cwd(), 'public/theme.js'), 'utf8');
const workshop = readFileSync(join(process.cwd(), 'workshop/index.html'), 'utf8');
const doc = new DOMParser().parseFromString(html, 'text/html');
const text = (selector: string) => [...doc.querySelectorAll(selector)].map((node) => node.textContent?.replace(/\s+/g, ' ').trim());

describe('the home page', () => {
  it('promises nothing of the old app', () => {
    expect(html).not.toMatch(/download|releases|demo\.html|silo-tour|How to use|\/media\//i);
    expect(text('.site-header nav a')).toEqual(['Why Silo', 'Workshop', 'GitHub ↗']);
    expect(text('.hero-actions a')).toEqual(['Follow the build', 'View source']);
    expect(doc.querySelector('.hero-actions a')?.getAttribute('href')).toBe('/workshop');
    expect(text('.platform-note')).toEqual(['Being rebuilt in the open']);
  });

  it('describes itself with the hero line', () => {
    const subline = text('.hero-description')[0];
    expect(subline).toBe('Give each agent its own Linux or macOS computer, with only the repositories and keys you pick.');
    expect(doc.querySelector('meta[name="description"]')?.getAttribute('content')).toBe(subline);
    expect(doc.querySelector('meta[property="og:description"]')?.getAttribute('content')).toBe(subline);
  });

  it('shows Why Silo as two columns with one line under the heading and no box', () => {
    const why = doc.getElementById('why') as HTMLElement;
    const [left, right] = [...(why.querySelector('.why-layout')?.children ?? [])];
    expect(left.querySelector('h2')?.textContent).toBe('Assume the agent is compromised.');
    expect(left.querySelector('.why-line')?.textContent).toBe('Limit what it can reach and what it is allowed to do.');
    expect(right.textContent).toMatch(/^\s*Agents are prompt-injected/);
    expect(why.querySelector('article, h3, table')).toBeNull();
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

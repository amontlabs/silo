import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const html = readFileSync(join(process.cwd(), 'index.html'), 'utf8');
const read = (file: string) => readFileSync(join(process.cwd(), file), 'utf8');
const css = ['src/style.css', 'src/cards.css'].map(read).join('\n');
const scripts = ['src/home.ts', 'src/pictures.ts', 'src/cards.ts', 'src/scene.ts'].map(read).join('\n');
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

  it('holds the way to the Workshop, then the hero, then the cards, then the notice', () => {
    expect([...doc.querySelectorAll('main > *')].map((node) => node.tagName + (node.className ? `.${node.className.split(' ')[0]}` : ''))).toEqual(['A.strip', 'SECTION.hero', 'SECTION.for', 'DIV.notice']);
    // The hero's place for the picture under the pitch stays empty.
    expect([...doc.querySelectorAll('.hero > *')].map((node) => node.tagName + (node.id ? `#${node.id}` : ''))).toEqual(['H1#hero-title', 'P']);
    expect(doc.querySelectorAll('.hero a')).toHaveLength(0);
    expect(doc.querySelector('main > a.strip')?.getAttribute('href')).toBe('/workshop');
    expect([...doc.querySelectorAll('script[type="module"]')].map((script) => script.getAttribute('src'))).toEqual(['/src/home.ts']);
  });

  it('says what Silo does in six cards: a picture, a label, a title, one paragraph', () => {
    expect(text('.for .title')).toEqual(['What Silo does']);
    expect(doc.querySelector('.for .title')?.tagName).toBe('H2');
    expect(doc.querySelector('.for')?.getAttribute('aria-labelledby')).toBe(doc.querySelector('.for .title')?.id);
    expect(text('.points .term')).toEqual(['full access', 'macOS or Linux', 'the boundary', 'the record', 'what needs you', 'going back']);
    expect(text('.points h3')).toEqual([
      'Agents have sudo, and nothing asks them for approval',
      'macOS or Linux, with a screen agents can drive',
      'Your rules, enforced outside the computer',
      'A record of everything agents did',
      'A short list of what needs you',
      'Silo takes you back to any moment',
    ]);
    expect(text('.points h3 + p')).toEqual([
      'Inside the computer, an agent installs a package or changes a system setting without a prompt. What it can reach outside is decided at the boundary.',
      'A computer is a virtual machine that runs macOS or Linux. An agent can open an app there and click through it.',
      'A connection that no rule allows is denied, and the attempt is recorded. Silo can propose rules from the hosts a computer actually used.',
      'It says what ran, which files changed, where the computer connected and what the model calls cost. What Silo saw itself is marked apart from what a program inside reported.',
      'The first screen ranks what needs you. An alert opens on what triggered it, and on the turn that led to it.',
      'Checkpoints are taken on their own while agents work. You restore one, or you continue from it in a fork while main keeps running.',
    ]);
    expect(doc.querySelectorAll('.points > li.card > .txt > .term + h3 + p:last-child')).toHaveLength(6);
    expect(doc.querySelectorAll('.points > li.card > svg.mini[aria-hidden="true"][viewBox="0 0 320 140"]')).toHaveLength(6);
    expect([...doc.querySelectorAll('.card')].map((card) => card.getAttribute('data-mini'))).toEqual(['full', 'screen', 'edge', 'record', 'needs', 'back']);
  });

  it('draws what the pictures say, and nothing the plan has not decided', () => {
    const picture = (name: string) => text(`.card[data-mini="${name}"] svg text`);
    // Three commands, each followed by its check, and no prompt, button or question.
    expect(picture('full')).toEqual(['main', '$ sudo apt install postgresql-17', '$ sudo systemctl restart postgresql', '$ npm install']);
    expect(doc.querySelectorAll('.card[data-mini="full"] svg .ok')).toHaveLength(3);
    expect(doc.querySelectorAll('.card[data-mini="full"] svg :is(button, a, input, .btnr)')).toHaveLength(0);
    expect(picture('screen')).toEqual(['macOS', 'Linux', 'agent']);
    expect(picture('edge')).toEqual(['main', 'crates.io', 'paste.rs', 'denied']);
    // A fact is a filled diamond and a report a hollow one: only the process is a report.
    expect(picture('record')).toEqual([
      'tool call', 'cargo build', 'process', 'cargo build', 'files', '214 changed', 'network', 'crates.io', 'model', '18 calls · $0.42', 'fact', 'report',
    ]);
    const kinds = (name: string) =>
      [...doc.querySelectorAll(`.card[data-mini="${name}"] svg .dm`)].map((mark) => (mark.classList.contains('fact') ? 'fact' : mark.classList.contains('op') ? 'opinion' : 'report'));
    expect(kinds('record')).toEqual(['fact', 'report', 'fact', 'fact', 'fact', 'fact', 'report']);
    expect(picture('needs')).toEqual(['1', 'paste.rs denied', '2', 'Codex asks for a host', '3', 'a run looks stuck', 'connection', 'paste.rs', 'turn 14', 'Codex']);
    expect(kinds('needs')).toEqual(['opinion', 'fact', 'opinion']);
    // The row of turn 14 and the line `a run looks stuck` carry the opinion mark, and the line leading to the row is dashed.
    expect(doc.querySelectorAll('.card[data-mini="needs"] svg .c2 .op, .card[data-mini="needs"] svg .g3 .op')).toHaveLength(2);
    expect(doc.querySelectorAll('.card[data-mini="needs"] svg .wd')).toHaveLength(1);
    expect(picture('back')).toEqual(['main', 'fork']);
    expect(doc.querySelectorAll('.card[data-mini="back"] svg .dm')).toHaveLength(5);
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
    expect(html).not.toMatch(/download|releases|demo\.html|silo-tour|How to use|\/media\//i);
  });

  it('says nothing the plan contradicts: no root, no row counts', () => {
    expect(doc.querySelector('main')?.textContent).not.toMatch(/\broot\b|\d+ rows/i);
  });

  it('takes every colour of the page and the pictures from the theme, so the dark theme changes them', () => {
    const body = html.slice(html.indexOf('<main'));
    expect(body).not.toMatch(/(?:fill|stroke|style)="[^"]*#[0-9a-f]{3,8}/i);
    expect(scripts).not.toMatch(/#[0-9a-f]{3,8}\b/i);
    expect(read('src/cards.css').replace(/\/\*[\s\S]*?\*\//g, '')).not.toMatch(/#[0-9a-f]{3,8}\b|rgba?\(|hsla?\(/i);
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
    // What style.css styles on purpose: the pictures' own frame, hidden until they are drawn, and a scene being put at a moment.
    for (const own of ['mini', 'jump']) page.delete(own);
    const inMarkup = [...doc.querySelectorAll('.mini, .mini *')].flatMap((part) => [...part.classList]);
    // In the scripts: the class names in a selector, those a cue adds or removes, and those of the wires it draws.
    const code = read('src/cards.ts') + read('src/scene.ts');
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

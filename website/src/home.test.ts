import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { homePage } from './film/test-support';

const html = homePage();
const read = (file: string) => readFileSync(join(process.cwd(), file), 'utf8');
const css = ['src/style.css', 'src/film/film.css', 'src/cards.css'].map(read).join('\n');
const filmFiles = readdirSync(join(process.cwd(), 'src/film')).filter((file) => file.endsWith('.ts') && !file.includes('test'));
const filmScripts = filmFiles.map((file) => read(`src/film/${file}`)).join('\n');
const scripts = [...['src/home.ts', 'src/pictures.ts', 'src/cards.ts'].map(read), filmScripts].join('\n');
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
    // The hero is the headline, the pitch and the film, and no link of its own, so the film starts high on the page.
    expect([...doc.querySelectorAll('.hero > *')].map((node) => node.tagName + (node.id ? `#${node.id}` : `.${node.className}`))).toEqual(['H1#hero-title', 'P.pitch', 'DIV.film']);
    expect(doc.querySelectorAll('.hero a')).toHaveLength(0);
    expect(doc.querySelector('main > a.strip')?.getAttribute('href')).toBe('/workshop');
    expect([...doc.querySelectorAll('script[type="module"]')].map((script) => script.getAttribute('src'))).toEqual(['/src/home.ts']);
  });

  it('tells the film in words for a screen reader, and has no title of its own', () => {
    // The headline above the film names it.
    expect(doc.querySelectorAll('.film :is(h2, h3, .title)')).toHaveLength(0);
    expect(doc.querySelector('.film figure')?.getAttribute('aria-labelledby')).toBe('hero-title');
    expect(doc.querySelector('.film figure')?.getAttribute('aria-describedby')).toBe('film-caption');
    // The stills are drawn for the eye; a screen reader gets the caption instead.
    for (const still of doc.querySelectorAll('.film figure > :not(figcaption)')) expect(still.getAttribute('aria-hidden')).toBe('true');
    expect(text('#film-caption')).toEqual([
      "The film follows one session in Silo. On the first run you copy your agent programs and your repositories into main, a virtual machine on your Mac. Agents work there, and Now shows what each one runs and what it spent. You limit main's network to the hosts it used. A secret is only a placeholder inside main, and its real value is put in at the boundary. A package that is too young is denied, and so is a request to paste.rs, which no rule allows. That denial waits in Needs you, and it opens in the Timeline on the process and the turn that led to it. A rule denies a forced Git push, and the model gets an error. In Disk you purge what a rebuild would bring back. You go back to a checkpoint in a fork, bring one file back to main, and drop the fork into the bin. Your changes are tested on a macOS computer and a Linux computer. An extension you install denies a request that waits at the boundary. An agent reads the record as text with the silo command.",
    ]);
  });

  it('gives the film a control a screen reader can name, and nothing to press in its still', () => {
    expect(text('.film-pause')).toEqual(['Pause']);
    expect(doc.querySelector('.film-progress')?.getAttribute('aria-label')).toBe('Shots of the film');
    expect(doc.querySelectorAll('.film figure :is(button, a, input, [tabindex], [role="button"])')).toHaveLength(0);
  });

  it('shows without script one still, for the window and for a phone: the end of Network', () => {
    expect(doc.querySelectorAll('.film figure > .film-window')).toHaveLength(1);
    expect(doc.querySelectorAll('.film figure > .film-phone')).toHaveLength(1);
    for (const still of ['.film-window', '.film-phone']) {
      const words = text(`${still} :is(text, span)`);
      for (const word of ['Network', 'STRIPE_KEY', 'stripe v2', 'allowed', 'registries v3', 'denied', 'paste.rs denied']) expect(words, still).toContain(word);
    }
  });

  it('says what Silo does in six cards, and three examples of the last under them', () => {
    expect(text('.for .title')).toEqual(['What Silo does']);
    expect(doc.querySelector('.for .title')?.tagName).toBe('H2');
    expect(doc.querySelector('.for')?.getAttribute('aria-labelledby')).toBe(doc.querySelector('.for .title')?.id);
    expect(text('.fc-grid .fc-term')).toEqual(['rules', 'secrets', 'record', 'screen', 'recipes', 'extensions']);
    expect(text('.fc-grid h3')).toEqual([
      'Agents have full access, and your rules decide at the boundary',
      'Agents use your keys without being able to read them',
      'What agents did, seen from outside their computer',
      'Agents click through Mac apps on a screen that is not yours',
      'Every computer comes from a recipe you can share',
      'Extensions read the record, alert you and deny requests',
    ]);
    expect(text('.fc-grid .fc-body')).toEqual([
      'Nothing inside the computer asks for approval. At the boundary, your rules allow, hold or deny each request. The default is deny.',
      'The computer holds a placeholder. Silo swaps in the real key on the way to a host you allowed. The key never enters the computer or the record.',
      'File changes, connections and model calls are facts Silo saw itself. What a program inside says is kept apart, since an agent can change it.',
      'Main is a macOS virtual machine with its own screen. Windows an agent opens there never take your focus or your screen.',
      'A recipe is text: a pinned base and a layer built by Nix. Silo makes a macOS and a Linux computer from it and runs your tests on each.',
      'One that runs code gets a computer of its own, with only the tools its manifest names. It can never allow what your rules deny.',
    ]);
    expect(text('.fc-examples .fc-term')).toEqual(['disk watch', 'tool call check', 'registry rules']);
    expect(text('.fc-examples .fc-body')).toEqual([
      'Every minute it reads /work on main, draws a panel and alerts you.',
      'It is asked about each tool call before the agent runs it. A denied call goes back to the model with an error.',
      'It has no computer, only rules that allow crates.io. You see them before you install it.',
    ]);
    expect(doc.querySelector('.fc-examples')?.getAttribute('aria-label')).toBe('Examples of extensions');
    expect(doc.querySelectorAll('.fc-examples h3')).toHaveLength(0);
    // Each is the rows the grid lines up: a picture, a label, then a title and a text, or a text alone.
    expect(doc.querySelectorAll('.fc-grid > li.fc-item > .fc-text > .fc-term + h3 + p.fc-body:last-child')).toHaveLength(6);
    expect(doc.querySelectorAll('.fc-examples > li.fc-item > .fc-text > .fc-term + p.fc-body:last-child')).toHaveLength(3);
    expect(doc.querySelectorAll('.fc-grid .fc-pic > svg[aria-hidden="true"][viewBox="0 0 376 188"]')).toHaveLength(6);
    expect(doc.querySelectorAll('.fc-examples .fc-pic > svg[aria-hidden="true"][viewBox="0 0 240 94"]')).toHaveLength(3);
    expect(doc.querySelectorAll('.fc-item[tabindex="0"] > .fc-fill[aria-hidden="true"]:first-child')).toHaveLength(9);
  });

  it('draws what the pictures say', () => {
    const picture = (name: string) => text(`.fc-item[data-picture="${name}"] svg text`);
    expect(picture('rules')).toEqual(['main', '$ sudo …', 'denied', 'registries', 'no rule', 'registry.npmjs.org', 'paste.rs']);
    expect(picture('secrets')).toEqual(['main', 'STRIPE_KEY', 'STRIPE_KEY', 'sk_live_••••', 'stripe', 'api.stripe.com']);
    expect(picture('record')).toEqual(['main', '$', 'fact · file changed', 'report · cargo build']);
    expect(picture('screen')).toEqual(['your Mac', 'main']);
    expect(picture('recipes')).toEqual(['base · pinned', 'nix · postgresql', 'pieces · skills', 'macOS', 'Linux']);
    expect(picture('extensions')).toEqual(['main', 'record', 'extension', 'alert']);
    expect(picture('disk')).toEqual(['main', '/work']);
    expect(picture('call')).toEqual(['check', 'git push --force', 'model', 'deny']);
    expect(picture('registry')).toEqual(['main', 'crates.io', 'allow']);
  });

  it('is at each picture\'s end without a script: only what travels and what covers is out of sight', () => {
    const cards = read('src/cards.css').replace(/\/\*[\s\S]*?\*\//g, '');
    const gone = [...cards.matchAll(/([^{}]+)\{([^{}]*)\}/g)].filter(([, , body]) => /opacity: *0|visibility|display: *none|scaleX\(0\)/.test(body)).map(([, selector]) => selector.trim());
    expect(gone.sort()).toEqual(['.for .fc-cover', '.for .fc-fill', '.for .fc-moving', '.for .fc-tree::after'].sort());
    // With reduced motion and without script there is nothing to switch off: the styles move nothing.
    expect(cards).not.toMatch(/animation|transition|@keyframes/);
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
    expect(['src/film/film.css', 'src/cards.css'].map(read).join('\n').replace(/\/\*[\s\S]*?\*\//g, '')).not.toMatch(/#[0-9a-f]{3,8}\b|rgba?\(|hsla?\(/i);
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
      // The film names the classes of what it draws several to a string.
      ...(filmScripts.match(/\bf-[a-z][a-z0-9-]*/g) ?? []),
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
    // What style.css styles on purpose: the film's root, hidden until it is drawn.
    page.delete('film');
    const inMarkup = [...doc.querySelectorAll('.film figure > :not(figcaption), .film figure > :not(figcaption) *, .fc-grid, .fc-grid *, .fc-examples, .fc-examples *, .fc-tree')].flatMap((part) => [...part.classList]);
    // The scripts of the cards name their classes in selectors, and add one.
    const inScripts = read('src/cards.ts').match(/\bfc-[a-z-]+/g) ?? [];
    expect([...new Set([...inMarkup, ...inScripts])].filter((name) => page.has(name))).toEqual([]);
  });

  it("keeps the film's styles under its root, and its names out of the rest of the page", () => {
    const selectorsOf = (file: string) =>
      (read(file).replace(/\/\*[\s\S]*?\*\//g, '').match(/[^{}]+(?=\{)/g) ?? [])
        .filter((selector) => !selector.trim().startsWith('@'))
        .flatMap((selector) => selector.split(',').map((one) => one.trim()));
    for (const selector of selectorsOf('src/film/film.css')) expect(selector).toMatch(/^(:root\[data-theme="dark"\] )?\.film(?![\w-])/);
    const classes = (file: string) => selectorsOf(file).flatMap((selector) => [...selector.matchAll(/\.([A-Za-z][\w-]*)/g)].map((match) => match[1]));
    expect(classes('src/film/film.css').filter((name) => !/^(film|f-|live$)/.test(name))).toEqual([]);
    // Outside its own stylesheet only its root is named, to hide it until it is drawn.
    expect(['src/site.css', 'src/style.css', 'src/cards.css'].flatMap(classes).filter((name) => /^(film-|f-)/.test(name))).toEqual([]);
  });

  it("keeps the cards' styles in their section, and their names out of the film and the rest of the page", () => {
    const selectorsOf = (file: string) =>
      (read(file).replace(/\/\*[\s\S]*?\*\//g, '').match(/[^{}]+(?=\{)/g) ?? [])
        .filter((selector) => !selector.trim().startsWith('@'))
        .flatMap((selector) => selector.split(',').map((one) => one.trim()));
    const classes = (selector: string) => [...selector.matchAll(/\.([A-Za-z][\w-]*)/g)].map((match) => match[1]);
    for (const selector of selectorsOf('src/cards.css')) {
      expect(selector).toMatch(/^(:root\[data-theme="dark"\] )?\.for /);
      expect(classes(selector).filter((name) => !/^(for|fc-|title$)/.test(name)), selector).toEqual([]);
    }
    const section = doc.querySelector('.for')!;
    expect([...section.querySelectorAll('*')].flatMap((el) => [...el.classList]).filter((name) => !/^(fc-|title$)/.test(name))).toEqual([]);
    expect([...section.querySelectorAll('[id]')].map((el) => el.id).filter((id) => !/^(fc-|for-title$)/.test(id))).toEqual([]);
    expect(['src/site.css', 'src/style.css', 'src/film/film.css'].flatMap(selectorsOf).flatMap(classes).filter((name) => name.startsWith('fc-'))).toEqual([]);
  });

  it('has a theme script with only what the two pages use', () => {
    expect(theme).not.toMatch(/iframe|postMessage|data-dark-source/);
    expect(html).toContain('data-theme-picker');
    expect(workshop).toContain('data-theme-picker');
  });
});

import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const html = readFileSync(join(process.cwd(), 'index.html'), 'utf8');
const read = (file: string) => readFileSync(join(process.cwd(), file), 'utf8');
const css = ['src/style.css', 'src/flow.css', 'src/cards.css'].map(read).join('\n');
const scripts = ['src/home.ts', 'src/pictures.ts', 'src/flow.ts', 'src/cards.ts', 'src/scene.ts'].map(read).join('\n');
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
    // The hero is the headline, the pitch and the flow, and no link of its own, so the flow starts high on the page.
    expect([...doc.querySelectorAll('.hero > *')].map((node) => node.tagName + (node.id ? `#${node.id}` : ''))).toEqual(['H1#hero-title', 'P', 'DIV#flow']);
    expect(doc.querySelectorAll('.hero a')).toHaveLength(0);
    expect(doc.querySelector('main > a.strip')?.getAttribute('href')).toBe('/workshop');
    expect([...doc.querySelectorAll('script[type="module"]')].map((script) => script.getAttribute('src'))).toEqual(['/src/home.ts']);
  });

  it('tells the flow in words for a screen reader, and has no title of its own', () => {
    // The headline above the flow names it.
    expect(doc.querySelectorAll('.flow :is(h2, h3, .title)')).toHaveLength(0);
    expect(doc.querySelector('.flow figure')?.getAttribute('aria-labelledby')).toBe('hero-title');
    // The picture is drawn for the eye; a screen reader gets the caption instead.
    expect(doc.querySelector('.stage')?.getAttribute('aria-hidden')).toBe('true');
    expect(doc.querySelector('.flow figure')?.getAttribute('aria-describedby')).toBe('flow-caption');
    expect(text('#flow-caption')).toEqual([
      'Three agents work in main, a virtual machine on your Mac, in two repositories. Each runs its commands with sudo, and nothing asks for approval. Silo writes down what happens. What it saw itself, such as a tool call, a file change, a connection or a model call, is a fact. What a program inside told it is a report, and it is marked apart. A request to a host that a rule allows passes. A request to a host that no rule allows is denied at the edge of main, and the attempt is recorded. A short list shows what needs you, and the denied connection opens on the process, the tool call and the turn that led to it. Another list ranks repeated work by what it cost. Checkpoints are taken on their own. You go back to the one before that turn and continue from it in a fork, while main keeps running. Before you drop the fork, Silo lists what is only there. You bring one file back to main, and the fork goes to the bin.',
    ]);
  });

  it('gives the flow controls a screen reader can name', () => {
    expect(text('.flow .ctl')).toEqual(['Pause']);
    const steps = [...doc.querySelectorAll('.steps li > button[type="button"]')];
    expect(steps.map((button) => button.getAttribute('aria-label'))).toEqual(['Main', 'Full access', 'Record', 'Boundary', 'Needs you', 'Cost', 'Go back', 'Drop']);
    expect(text('.steps .lb')).toEqual(steps.map((button) => button.getAttribute('aria-label')));
    // What is drawn inside the picture is not a control: nothing in it takes the keyboard.
    expect(doc.querySelectorAll('.stage :is(button, a, input, [tabindex])')).toHaveLength(0);
  });

  it('draws in the flow what the plan says, with the kind of each line of the record', () => {
    const lines = (selector: string) =>
      [...doc.querySelectorAll(`.stage ${selector}`)].map((line) => {
        const mark = line.querySelector('.mk');
        const kind = !mark?.matches('svg') ? '' : mark.classList.contains('fact') ? 'fact' : mark.classList.contains('opinion') ? 'opinion' : 'report';
        return `${kind} ${line.textContent?.replace(/\s+/g, ' ').trim()}`.trim();
      });
    expect(text('.stage .main .who')).toEqual(['Claude Code~/api', 'Codex~/web', 'Claude Code~/api']);
    expect(text('.stage .main .cmd')).toEqual(['$ sudo apt install postgresql-17', '$ npm install', '$ curl paste.rs', '$ cargo build']);
    // Process names are reports and run names are opinions.
    expect(lines('.rr')).toEqual([
      'fact tool callcargo build',
      'report processcargo build',
      'fact files214 changed',
      'fact model18 calls · $0.42',
      'opinion by Claude Code, run 2',
      'fact networkcrates.io',
      'fact networkpaste.rsdenied',
    ]);
    expect(lines('.legend span')).toEqual(['fact fact', 'report report', 'opinion opinion']);
    expect(lines('.cc')).toEqual(['fact connectionpaste.rs', 'report processcurl', 'fact tool callcurl paste.rs', 'turn 14Codex']);
    expect(lines('.nr')).toEqual(['1paste.rs denied', '2Codex asks for a host', 'opinion 3a run looks stuck']);
    expect(text('.stage .cr')).toEqual(['npm install41 times in 12 runs38 min2.1 GB', 'cargo build9 times12 min1.1 GB', 'apt update7 times3 min0.2 GB']);
    expect(text('.stage .host')).toEqual(['paste.rs', 'crates.io']);
    expect(text('.stage .fn')).toEqual(['notes.mdnot committedBring back', 'CLAUDE.mdeditedBring back']);
    expect(text('.stage :is(.mac-name, .cp-name, .hd b, .denied, .bin, .file, .pb.drop)')).toEqual([
      'your Mac', 'turn 14', 'main', 'record', 'paste.rs denied', 'needs you', 'cost', 'denied', 'bin', 'fork', 'Drop', 'only here', 'notes.md',
    ]);
    expect(doc.querySelector('.stage')?.textContent).not.toMatch(/sandbox|safe|secure|proven|dashboard|monitor|observab|merge|diff/i);
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
    expect(['src/flow.css', 'src/cards.css'].map(read).join('\n').replace(/\/\*[\s\S]*?\*\//g, '')).not.toMatch(/#[0-9a-f]{3,8}\b|rgba?\(|hsla?\(/i);
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
    // What style.css styles on purpose: the pictures' own frames, hidden until they are drawn, and a scene being put at a moment.
    for (const own of ['stage', 'steps', 'ctl', 'mini', 'jump']) page.delete(own);
    const inMarkup = [...doc.querySelectorAll('.stage, .stage *, .steps, .steps *, .mini, .mini *')].flatMap((part) => [...part.classList]);
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

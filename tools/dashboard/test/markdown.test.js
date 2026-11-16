import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { renderMarkdown, safeHref } from '../src/markdown.js';
import { createServer } from '../src/server.js';
import { Store } from '../src/store.js';
import { JOURNEYS, makeRepo, request, tempDir } from './helpers.js';

// The five ways a document could fake owner controls or change the page.
const INJECTIONS = {
  'button[data-post]': '<button class="btn primary" data-post="/api/docs/seen" data-body="{}">Agree</button>',
  'form[data-api]': '<form data-api="/api/items/1/replies" data-verdict="agree"><textarea>x</textarea><button type="submit">Send</button></form>',
  'data-region': '<div data-region="for-you">Nothing is waiting for you.</div>',
  'meta refresh': '<meta http-equiv="refresh" content="0;url=https://example.com/">',
  style: '<style>[data-region="for-you"] { display: none }</style>',
};
const RAW_TAG = /<(?:button|form|textarea|meta|style)\b|<div data-region/;

test('raw HTML is escaped in every Markdown source', () => {
  for (const [name, html] of Object.entries(INJECTIONS)) {
    for (const source of [html, `Text\n\n${html}\n\nMore`, `Inline ${html} here`]) {
      const out = renderMarkdown(source);
      assert.doesNotMatch(out, RAW_TAG, name);
      assert.match(out, /&lt;(?:button|form|div|meta|style)/, name);
    }
  }
});

test('documents and private notes cannot add controls, regions, refreshes or styles to the page', async () => {
  const all = Object.values(INJECTIONS).join('\n\n');
  const repo = makeRepo({ product: `# Silo\n\n${all}\n`, decisions: `# Decisions\n\n- One.\n\n${all}\n`, journeys: `${JOURNEYS}\n${all}\n` });
  const home = tempDir();
  fs.mkdirSync(path.join(home, 'private'));
  fs.writeFileSync(path.join(home, 'private', 'note.md'), `# Note\n\n${all}\n`);
  const store = new Store(home);
  const server = createServer({ store, repoRoot: repo });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  try {
    const html = (await request(server.address().port)).body.toString();
    for (const region of ['product', 'journeys', 'decisions', 'private']) {
      const section = new RegExp(`<section class="view" id="${region}"[\\s\\S]*?</section>`).exec(html)[0];
      // Each injection shows as text, never as markup.
      // Product shows only the hero paragraph of a file without sections, and
      // Decisions only list items, so the full injection text shows in the others.
      if (region !== 'decisions') assert.match(section, /&lt;button class=&quot;btn primary&quot; data-post=/, region);
      if (region === 'journeys' || region === 'private') assert.match(section, /&lt;form data-api=/, region);
      assert.doesNotMatch(section, /<div data-region|<meta|<style|<textarea>x/, region);
      assert.doesNotMatch(section, /data-post="|data-api="/, region);
      // Every button, form and textarea left is one the server renders.
      for (const tag of section.match(/<button\b[^>]*>/g) || []) assert.match(tag, /^<button class="[^"]*" type="(?:button|submit)"/, region);
      for (const tag of section.match(/<form\b[^>]*>/g) || []) assert.match(tag, /^<form class="say\s*"\s+data-note data-draft="note-/, region);
      for (const tag of section.match(/<textarea\b[^>]*>/g) || []) assert.match(tag, /^<textarea name="text" rows="3" placeholder="Your note/, region);
      // The only controls here are the server's own note wrappers.
      for (const tag of section.match(/<[^>]*data-controls[^>]*>/g) || []) assert.match(tag, /^<div class="note-controls" data-controls data-anchor="(?:journeys|product)#/, region);
    }
    assert.doesNotMatch(html, /<meta http-equiv/);
    assert.doesNotMatch(html, /<style/);
    assert.equal(html.match(/data-region="for-you"/g).length, 1);
    // The only posting control on this page is the server's own Mark all as seen.
    assert.equal(html.match(/data-post="/g).length, 1);
  } finally {
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
    store.close();
  }
});

test('links and images: entity-encoded and reference forms of unsafe schemes render as text', () => {
  const cases = {
    entity: '[a](javascript&#58;alert(1))',
    reference: '[a][x]\n\n[x]: javascript&#58;alert(2)',
    'data image': '![a](data&#58;text/html,x)',
    'hex entity': '[a](&#x6a;avascript:alert(3))',
    'named entity': '[a](javascript&colon;alert(4))',
    'no semicolon': '[a](javascript&#58alert(5))',
    'upper case': '[a](JAVASCRIPT:alert(6))',
    'tab inside': '[a](java&Tab;script:alert(7))',
    'data link': '[a](data:text/html;base64,PHNjcmlwdD4=)',
    'vbscript image': '![a](vbscript:x)',
    'protocol relative': '[a](//example.com/x)',
    backslash: '[a](\\\\example.com)',
  };
  for (const [name, source] of Object.entries(cases)) {
    const out = renderMarkdown(source);
    assert.doesNotMatch(out, /<a |<img /, name);
    assert.match(out, /<p>a<\/p>/, name);
  }
});

test('links and images: http, https, mailto, relative and anchors are kept', () => {
  assert.match(renderMarkdown('[a](https://example.com/?x=1&y=2)'), /<a href="https:\/\/example\.com\/\?x=1&amp;y=2">a<\/a>/);
  assert.match(renderMarkdown('[a](http&#58;//example.com)'), /<a href="http:\/\/example\.com">a<\/a>/);
  assert.match(renderMarkdown('[a](mailto:owner@example.com)'), /<a href="mailto:owner@example\.com">/);
  assert.match(renderMarkdown('[a](docs/x.md)'), /<a href="docs\/x\.md">/);
  assert.match(renderMarkdown('[a](#section)'), /<a href="#section">/);
  assert.match(renderMarkdown('[a](/attachments/1)'), /<a href="\/attachments\/1">/);
  assert.match(renderMarkdown('![shot](/attachments/2 "Title")'), /<img src="\/attachments\/2" alt="shot" title="Title">/);
  assert.match(renderMarkdown('[a][r]\n\n[r]: https://example.com'), /<a href="https:\/\/example\.com">a<\/a>/);
  assert.equal(safeHref('foo/bar:baz'), 'foo/bar:baz');
  assert.equal(safeHref('foo:bar'), null);
});

test('journey steps: the renderer makes the chips', () => {
  const out = renderMarkdown(
    [
      '- [built] Install',
      '- [Partial] Start',
      '- [missing] Tour',
      '- [untested] Restore **now**',
      '- [later] Not a status',
      '- [built]Glued',
      '',
      '1. [built] Ordered stays text',
      '',
      '```',
      '- [built] in a fence',
      '```',
      '',
      '- loose',
      '',
      '- [missing] Loose step',
    ].join('\n'),
    { steps: true },
  );
  assert.match(out, /<li><span class="chip step-built">built<\/span> Install<\/li>/);
  assert.match(out, /<span class="chip step-partial">partial<\/span> Start/);
  assert.match(out, /<span class="chip step-missing">missing<\/span> Tour/);
  assert.match(out, /<span class="chip step-untested">untested<\/span> Restore <strong>now<\/strong>/);
  assert.match(out, /<li>\[later\] Not a status<\/li>/);
  assert.match(out, /<li>\[built\]Glued<\/li>/);
  assert.match(out, /<li>\[built\] Ordered stays text<\/li>/);
  assert.match(out, /<code>- \[built\] in a fence\n<\/code>/);
  assert.match(out, /<span class="chip step-missing">missing<\/span> Loose step/);
  assert.doesNotMatch(renderMarkdown('- [built] Install'), /chip/, 'chips only where asked');
  assert.match(renderMarkdown('```mermaid\ngraph TD; A-->B<script>\n```'), /<pre class="mermaid">graph TD; A--&gt;B&lt;script&gt;<\/pre>/);
});

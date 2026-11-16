// Server-side Markdown for every source on the page: product documents,
// private notes, item bodies and replies. Raw HTML is always escaped, so a
// document cannot add elements, attributes or styles of its own. Links and
// images keep to http, https, mailto and relative targets, checked on the
// href the browser will see. Journey step chips and mermaid blocks are
// produced here, by the renderer.
import { Marked } from 'marked';
import { STEP_STATUSES } from './docs.js';
import { esc } from './html.js';

const STEP_PREFIX = new RegExp(`^\\[(${STEP_STATUSES.join('|')})\\](?=\\s|$)\\s*`, 'i');

const NAMED_ENTITIES = {
  amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", colon: ':', tab: '\t', newline: '\n', sol: '/',
  lpar: '(', rpar: ')', period: '.', comma: ',', semi: ';', equals: '=', quest: '?', num: '#', nbsp: ' ',
};

function decodeEntities(text) {
  return text.replace(/&(?:#(\d+)|#x([0-9a-f]+)|([a-z]+));?/gi, (whole, dec, hex, name) => {
    if (dec || hex) {
      const code = dec ? Number(dec) : parseInt(hex, 16);
      return code > 0 && code <= 0x10ffff ? String.fromCodePoint(code) : '';
    }
    const value = NAMED_ENTITIES[name.toLowerCase()];
    return value === undefined ? whole : value;
  });
}

/**
 * The href a link or image may carry, or null. Entities are decoded and the
 * characters browsers drop from URLs are removed first; the result is what
 * the page prints, so the browser sees exactly the checked value.
 */
export function safeHref(href) {
  const url = decodeEntities(String(href ?? '')).replace(/[\u0000- \u007f-\u009f]/g, '');
  if (/^(?:https?|mailto):/i.test(url)) return url;
  // Relative or anchor: no scheme, that is no colon before the first / ? or #.
  if (/^[^:/?#\\]*(?:[/?#]|$)/.test(url) && !url.startsWith('//') && !url.includes('\\')) return url;
  return null;
}

const titleAttr = (title) => (title ? ` title="${esc(title)}"` : '');

const renderer = {
  html({ text }) {
    return esc(text);
  },
  code({ text, lang }) {
    if ((lang || '').trim().split(/\s+/)[0] === 'mermaid') return `<pre class="mermaid">${esc(text)}</pre>\n`;
    return false;
  },
  link({ href, title, tokens }) {
    const inner = this.parser.parseInline(tokens);
    const url = safeHref(href);
    return url === null ? inner : `<a href="${esc(url)}"${titleAttr(title)}>${inner}</a>`;
  },
  image({ href, title, text }) {
    const url = safeHref(href);
    return url === null ? esc(text) : `<img src="${esc(url)}" alt="${esc(text)}"${titleAttr(title)}>`;
  },
};

const stepChip = {
  name: 'stepChip',
  renderer: ({ status }) => `<span class="chip step-${status}">${status}</span> `,
};

// Turns the leading [status] of an unordered list item into a stepChip token.
function markSteps(token) {
  if (token.type !== 'list' || token.ordered) return;
  for (const item of token.items) {
    const block = item.tokens[0];
    if (!block || (block.type !== 'text' && block.type !== 'paragraph') || !block.tokens?.length) continue;
    const first = block.tokens[0];
    if (first.type !== 'text') continue;
    const match = STEP_PREFIX.exec(first.text);
    if (!match) continue;
    first.text = first.text.slice(match[0].length);
    first.raw = first.raw.slice(Math.min(match[0].length, first.raw.length));
    block.tokens.unshift({ type: 'stepChip', raw: '', status: match[1].toLowerCase() });
  }
}

const plain = new Marked({ gfm: true, renderer, extensions: [stepChip] });
const withSteps = new Marked({ gfm: true, renderer, extensions: [stepChip], walkTokens: markSteps });

/** Markdown to HTML. `steps: true` renders journey step markers as chips. */
export function renderMarkdown(markdown, { steps = false } = {}) {
  return (steps ? withSteps : plain).parse(markdown ?? '');
}

export const lexMarkdown = (markdown) => plain.lexer(markdown ?? '');

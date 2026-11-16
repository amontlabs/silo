// Server-side Markdown for every source on the page: product documents,
// private notes, item bodies and replies. Raw HTML is always escaped, so a
// document cannot add elements, attributes or styles of its own. Links and
// images keep to http, https, mailto and relative targets, checked on the
// href the browser will see. Mermaid blocks are produced here, by the
// renderer.
import { Marked } from 'marked';
import { esc } from './html.js';

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

const plain = new Marked({ gfm: true, renderer });

/** Markdown to HTML. */
export function renderMarkdown(markdown) {
  return plain.parse(markdown ?? '');
}

/** One line of Markdown to inline HTML (no paragraph), with the same escaping and link rules. */
export function renderInline(markdown) {
  return plain.parseInline(String(markdown ?? '').replace(/\s*\n\s*/g, ' '));
}

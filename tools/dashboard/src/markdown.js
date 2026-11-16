// Server-side Markdown. Product documents come from the repository and render
// as written; item bodies and replies come from agents and the owner, so raw
// HTML in them is escaped and links keep to a few safe schemes.
import { Marked } from 'marked';
import { esc } from './html.js';

const SAFE_URL = /^(?:https?:|mailto:|#|\/(?!\/)|[^:]*$)/i;

function make({ untrusted }) {
  const renderer = {
    code({ text, lang }) {
      if ((lang || '').trim().split(/\s+/)[0] === 'mermaid') return `<pre class="mermaid">${esc(text)}</pre>\n`;
      return false;
    },
  };
  if (untrusted) renderer.html = ({ text }) => esc(text);
  return new Marked({
    gfm: true,
    renderer,
    walkTokens(token) {
      if (!untrusted) return;
      if ((token.type === 'link' || token.type === 'image') && !SAFE_URL.test(token.href.replace(/[\s\u0000-\u001f]/g, ''))) token.href = '#';
    },
  });
}

const trusted = make({ untrusted: false });
const untrusted = make({ untrusted: true });

export const renderDoc = (markdown) => trusted.parse(markdown ?? '');
export const renderUntrusted = (markdown) => untrusted.parse(markdown ?? '');
export const lexDoc = (markdown) => trusted.lexer(markdown ?? '');

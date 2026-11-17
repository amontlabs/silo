// A feature's notes, from Markdown to elements: headings, paragraphs, lists, http(s) links, inline code and
// code blocks. Anything else stays text; nothing here is ever inserted as HTML.
import type { ReactNode } from 'react';
import { inline } from './RichText';

type Block =
  | { kind: 'heading'; level: number; text: string }
  | { kind: 'paragraph'; text: string }
  | { kind: 'code'; text: string }
  | { kind: 'list'; ordered: boolean; items: Item[] };

interface Item {
  text: string;
  children: Block[];
}

const HEADING = /^(#{1,6})\s+(.*?)\s*#*\s*$/;
const ITEM = /^(\s*)([-*+]|\d{1,9}[.)])\s+(.*)$/;
const FENCE = /^\s*(```|~~~)/;

/** A link to anything but http or https keeps only its label. */
const plainLinks = (text: string) => text.replace(/\[([^\]\n]+)\]\((?!https?:\/\/)[^)\n]*\)/g, '$1');

/** The blocks of a Markdown text, in one pass over its lines. */
export function parseMarkdown(source: string): Block[] {
  const lines = source.replace(/\r\n?/g, '\n').split('\n');
  const blocks: Block[] = [];
  let paragraph: string[] = [];
  // The open lists, innermost last, each with the indent of its items.
  let lists: { indent: number; block: Extract<Block, { kind: 'list' }> }[] = [];
  const flush = () => {
    if (paragraph.length) blocks.push({ kind: 'paragraph', text: paragraph.join(' ') });
    paragraph = [];
  };
  const closeLists = () => { lists = []; };
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    const fence = FENCE.exec(line);
    if (fence) {
      flush();
      closeLists();
      const body: string[] = [];
      for (i += 1; i < lines.length && !lines[i].trimStart().startsWith(fence[1]); i += 1) body.push(lines[i]);
      blocks.push({ kind: 'code', text: body.join('\n') });
      continue;
    }
    if (!line.trim()) {
      flush();
      continue;
    }
    const heading = HEADING.exec(line);
    if (heading) {
      flush();
      closeLists();
      blocks.push({ kind: 'heading', level: heading[1].length, text: heading[2] });
      continue;
    }
    const item = ITEM.exec(line);
    if (item) {
      flush();
      const indent = item[1].replace(/\t/g, '    ').length;
      const ordered = /\d/.test(item[2]);
      while (lists.length && lists[lists.length - 1].indent > indent) lists.pop();
      const top = lists[lists.length - 1];
      if (top && top.indent === indent && top.block.ordered === ordered) {
        top.block.items.push({ text: item[3], children: [] });
      } else {
        const block: Extract<Block, { kind: 'list' }> = { kind: 'list', ordered, items: [{ text: item[3], children: [] }] };
        const parent = top && top.indent < indent ? top.block.items[top.block.items.length - 1] : null;
        if (parent) parent.children.push(block);
        else {
          lists = [];
          blocks.push(block);
        }
        lists.push({ indent, block });
      }
      continue;
    }
    // A line under a list item, indented, continues it; anything else is a paragraph.
    const top = lists[lists.length - 1];
    if (top && /^\s/.test(line) && !paragraph.length) {
      const last = top.block.items[top.block.items.length - 1];
      last.text = `${last.text} ${line.trim()}`;
      continue;
    }
    closeLists();
    paragraph.push(line.trim());
  }
  flush();
  return blocks;
}

function render(blocks: Block[], key: string): ReactNode[] {
  return blocks.map((block, index) => {
    const id = `${key}-${index}`;
    switch (block.kind) {
      case 'heading': {
        const level = Math.min(6, block.level + 2);
        const Tag = `h${level}` as 'h3' | 'h4' | 'h5' | 'h6';
        return <Tag key={id} className="ws-md-h">{inline(plainLinks(block.text), id)}</Tag>;
      }
      case 'code': return <pre key={id}><code>{block.text}</code></pre>;
      case 'list': {
        const Tag = block.ordered ? 'ol' : 'ul';
        return (
          <Tag key={id}>
            {block.items.map((item, i) => <li key={`${id}-${i}`}>{inline(plainLinks(item.text), `${id}-${i}`)}{item.children.length ? render(item.children, `${id}-${i}`) : null}</li>)}
          </Tag>
        );
      }
      default: return <p key={id}>{inline(plainLinks(block.text), id)}</p>;
    }
  });
}

/** Notes as elements. A first heading that only says "Notes" is left out: the notes sit under that name already. */
export function Markdown({ text }: { text: string }) {
  const blocks = parseMarkdown(text);
  if (blocks[0]?.kind === 'heading' && blocks[0].text.trim().toLowerCase() === 'notes') blocks.shift();
  return <div className="ws-md">{render(blocks, 'md')}</div>;
}

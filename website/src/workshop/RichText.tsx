// Text written on GitHub, shown as text: paragraphs, inline code and http(s) links.
// Nothing here is ever inserted as HTML.
import type { ReactNode } from 'react';

const TOKEN = /(`[^`\n]+`)|\[([^\]\n]+)\]\((https?:\/\/[^\s)]+)\)|(https?:\/\/[^\s<>()]+[^\s<>().,;:!?'"])/g;

function safeHref(url: string): string | null {
  try {
    const parsed = new URL(url);
    return parsed.protocol === 'https:' || parsed.protocol === 'http:' ? parsed.href : null;
  } catch {
    return null;
  }
}

function inline(text: string, key: string): ReactNode[] {
  const out: ReactNode[] = [];
  let last = 0;
  let index = 0;
  for (const match of text.matchAll(TOKEN)) {
    const start = match.index ?? 0;
    if (start > last) out.push(text.slice(last, start));
    const [whole, code, label, labelled, bare] = match;
    const href = safeHref(labelled ?? bare ?? '');
    if (code) out.push(<code key={`${key}-${index}`}>{code.slice(1, -1)}</code>);
    else if (href) {
      out.push(
        <a key={`${key}-${index}`} href={href} rel="noopener noreferrer nofollow ugc" target="_blank">
          {label ?? bare}
        </a>,
      );
    } else out.push(whole);
    last = start + whole.length;
    index += 1;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

export function RichText({ text, className }: { text: string; className?: string }) {
  const paragraphs = text
    .replace(/\r\n/g, '\n')
    .split(/\n{2,}/)
    .map((part) => part.trim())
    .filter(Boolean);
  return (
    <>
      {paragraphs.map((paragraph, i) => (
        <p className={className} key={i}>
          {paragraph.split('\n').flatMap((line, j) => (j === 0 ? inline(line, `${i}-${j}`) : [<br key={`${i}-${j}-br`} />, ...inline(line, `${i}-${j}`)]))}
        </p>
      ))}
    </>
  );
}

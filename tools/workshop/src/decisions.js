// DECISIONS.md list items: which ones already carry an issue reference
// " (#N)", the text of the others, and appending a reference to one item
// without touching any other byte.

const REFERENCE = / \(#(\d+)\)\s*$/;

/**
 * The top-level list items: { start, end, text, issue } where start and end
 * are line indexes (end inclusive; indented continuation lines belong to the
 * item) and text is the item without its "- " marker, lines joined by spaces.
 */
export function listItems(lines) {
  const items = [];
  let fence = false;
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i].replace(/\r$/, '');
    if (/^(```|~~~)/.test(line)) fence = !fence;
    if (fence || !/^[-*] \S/.test(line)) continue;
    let end = i;
    while (end + 1 < lines.length && /^\s+\S/.test(lines[end + 1]) && !/^\s*[-*] /.test(lines[end + 1])) end += 1;
    const parts = lines.slice(i, end + 1).map((l, k) => (k === 0 ? l.slice(2) : l).replace(/\r$/, '').trim());
    const joined = parts.join(' ');
    const reference = joined.match(REFERENCE);
    items.push({
      start: i,
      end,
      text: reference ? joined.replace(REFERENCE, '') : joined,
      issue: reference ? Number(reference[1]) : null,
    });
    i = end;
  }
  return items;
}

/** Every issue number referenced anywhere in the file as "(#N)". */
export function referencedIssues(text) {
  return new Set([...text.matchAll(/\(#(\d+)\)/g)].map((m) => Number(m[1])));
}

/** Appends " (#N)" to the last line of an item, before trailing spaces and a CR. */
export function appendReference(lines, item, number) {
  const copy = lines.slice();
  const last = copy[item.end];
  const match = last.match(/^(.*?)([ \t]*\r?)$/);
  copy[item.end] = `${match[1]} (#${number})${match[2]}`;
  return copy;
}

/** The title: the text cut at 100 characters on a word, with an ellipsis when cut. */
export function decisionTitle(text, limit = 100) {
  if (text.length <= limit) return text;
  const room = text.slice(0, limit - 1);
  const space = room.lastIndexOf(' ');
  const cut = (space > limit / 2 ? room.slice(0, space) : room).replace(/[\s,;:·.-]+$/, '');
  return `${cut}…`;
}

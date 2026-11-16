// The product documents (docs/product/*.md): read live from the checkout,
// split into sections, and compared with the commit the owner last marked as
// seen. Nothing here writes to the checkout.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

export const DOC_FOLDER = 'docs/product';
export const DOC_FILES = Object.freeze({
  product: 'PRODUCT.md',
  decisions: 'DECISIONS.md',
  restart: 'RESTART.md',
});

const FENCE = /^\s{0,3}(```|~~~)/;

export function slugify(text) {
  return String(text)
    .normalize('NFKD')
    .replace(/[̀-ͯ]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
}

// The one definition of an id or key the dashboard accepts: section keys in
// anchors, and the list entry an item names (--entry).
export const ID_SHAPE = /^[a-z0-9-]{1,80}$/;
export const MAX_ID = 80;
export const MAX_ANCHOR_TEXT = 200;

// `base` cut to fit ID_SHAPE, made unique among `used` with -2, -3, ...
// (in the order asked), and added to `used`.
export function uniqueId(base, used, fallback) {
  const clean = (text) => text.slice(0, MAX_ID).replace(/-+$/, '');
  const root = clean(base) || fallback;
  let id = root;
  for (let n = 2; used.has(id); n += 1) id = `${clean(root.slice(0, MAX_ID - String(n).length - 1))}-${n}`;
  used.add(id);
  return id;
}

// The text the page shows for an anchor and the server compares as sent:
// plain, trimmed first, then cut to MAX_ANCHOR_TEXT characters (so a cut may
// end on a space). The one definition both sides use.
export const anchorText = (text) => String(text).replace(/[`*_]/g, '').trim().slice(0, MAX_ANCHOR_TEXT);

// Reads one of the known documents. A missing file, a missing folder, or
// a path that resolves outside docs/product gives null.
export function readDoc(repoRoot, fileName) {
  if (!Object.values(DOC_FILES).includes(fileName)) return null;
  try {
    const folder = fs.realpathSync(path.join(repoRoot, DOC_FOLDER));
    const file = fs.realpathSync(path.join(folder, fileName));
    if (path.dirname(file) !== folder) return null;
    return fs.readFileSync(file, 'utf8').replace(/\r\n/g, '\n');
  } catch {
    return null;
  }
}

// Calls visit(line, inFence) for each line, tracking fenced code.
function eachLine(text, visit) {
  let fence = null;
  const lines = text.split('\n');
  return lines.map((line, index) => {
    const marker = FENCE.exec(line);
    let inFence = fence !== null;
    if (marker) {
      if (fence === null) {
        fence = marker[1];
        inFence = true;
      } else if (marker[1] === fence) {
        fence = null;
      }
    }
    return visit(line, inFence, index);
  });
}

// { preamble, sections: [{ heading, key, text }] } split at `## ` headings
// outside code fences. `text` includes the heading line.
export function splitSections(text) {
  const preamble = [];
  const sections = [];
  let current = null;
  eachLine(text ?? '', (line, inFence) => {
    if (!inFence && /^## +\S/.test(line)) {
      current = { heading: line.replace(/^## +/, '').replace(/\s+#+\s*$/, '').trim(), lines: [line] };
      sections.push(current);
    } else if (current) {
      current.lines.push(line);
    } else {
      preamble.push(line);
    }
  });
  const used = new Set();
  return {
    preamble: preamble.join('\n').replace(/\s+$/, ''),
    sections: sections.map((section) => ({
      heading: section.heading,
      key: uniqueId(slugify(section.heading), used, 'section'),
      text: section.lines.join('\n').replace(/\s+$/, ''),
    })),
  };
}

const LIST_ITEM = /^(\s*)(?:[-*+]|\d{1,9}[.)])\s+/;
const BLOCK_START = /^\s{0,3}(?:#{1,6}\s|```|~~~|>|<!--)/;
const indentOf = (line) => /^\s*/.exec(line)[0].replace(/\t/g, '    ').length;
const blank = (line) => line.trim() === '';

function dedent(lines) {
  const widths = lines.filter((line) => !blank(line)).map(indentOf);
  const cut = widths.length ? Math.min(...widths) : 0;
  return lines.map((line) => line.replace(/\t/g, '    ').slice(Math.min(cut, indentOf(line)))).join('\n');
}

// The index of the first ". " outside code spans, or -1.
function sentenceEnd(text) {
  let inCode = false;
  for (let i = 0; i < text.length - 1; i += 1) {
    if (text[i] === '`') inCode = !inCode;
    else if (!inCode && text[i] === '.' && text[i + 1] === ' ') return i;
  }
  return -1;
}

/**
 * A list item that starts at lines[start], as { paragraph, nested, end }:
 * the item's first paragraph (its first line and the lines that continue it)
 * joined with spaces, the indented content under it, dedented, and the index
 * after its last line.
 */
function listItemAt(lines, start, firstText) {
  const indent = indentOf(lines[start]);
  const paragraph = [firstText.trim()];
  let j = start + 1;
  while (j < lines.length && !blank(lines[j]) && !LIST_ITEM.test(lines[j]) && !BLOCK_START.test(lines[j])) {
    paragraph.push(lines[j].trim());
    j += 1;
  }
  const nested = [];
  while (j < lines.length) {
    if (blank(lines[j])) {
      let k = j;
      while (k < lines.length && blank(lines[k])) k += 1;
      if (k < lines.length && indentOf(lines[k]) > indent) {
        nested.push(...lines.slice(j, k));
        j = k;
        continue;
      }
      break;
    }
    if (indentOf(lines[j]) <= indent) break;
    nested.push(lines[j]);
    j += 1;
  }
  return { paragraph: paragraph.join(' '), nested: dedent(nested).trim(), end: j };
}

/**
 * Every place an owner note can point to in the current documents: the
 * sections of PRODUCT.md, as anchor -> { text (what the page shows and the
 * server compares) }.
 */
export function noteTargets(docs) {
  const targets = new Map();
  if (docs.product != null) {
    for (const tile of productLayout(docs.product).tiles) targets.set(`product#${tile.key}`, { text: anchorText(tile.heading) });
  }
  return targets;
}

// ---- where the work stands (RESTART.md) ----

export const RESTART_HEADING = 'where the work stands';
export const STEP_STATES = Object.freeze(['done', 'now', 'next', 'then']);

/**
 * The ordered list under the RESTART.md heading "Where the work stands", as
 * [{ state, text }] in file order. Each item starts with a state word and a
 * colon (Done, Now, Next or Then, any case); `state` is that word in lower
 * case and `text` the rest. An item with another first word has state
 * 'other' and keeps its whole text. Null when the file, the heading or the
 * list is missing.
 */
export function parseRestart(text) {
  if (text == null) return null;
  const section = splitSections(text).sections.find((entry) => entry.heading.trim().toLowerCase() === RESTART_HEADING);
  if (!section) return null;
  const lines = section.text.split('\n').slice(1);
  const steps = [];
  let fence = null;
  for (let i = 0; i < lines.length; i += 1) {
    const marker = FENCE.exec(lines[i]);
    if (marker) {
      if (fence === null) fence = marker[1];
      else if (marker[1] === fence) fence = null;
      continue;
    }
    if (fence !== null) continue;
    const match = /^(\s*)\d{1,9}[.)]\s+/.exec(lines[i]);
    if (!match || indentOf(lines[i]) > 3) continue;
    const item = listItemAt(lines, i, lines[i].slice(match[0].length));
    i = item.end - 1;
    const word = /^([A-Za-z]+):\s*/.exec(item.paragraph);
    const state = word && STEP_STATES.includes(word[1].toLowerCase()) ? word[1].toLowerCase() : 'other';
    steps.push({ state, text: state === 'other' ? item.paragraph : item.paragraph.slice(word[0].length) });
  }
  return steps.length ? steps : null;
}

// The first sentence of a Markdown line (up to the first ". " outside code
// spans, the period kept), or the whole line.
export function firstSentence(text) {
  const end = sentenceEnd(text);
  return end === -1 ? text : text.slice(0, end + 1);
}

// ---- decisions ----

const DECISION_DATE = /^(\d{4})-(\d{2})-(\d{2})$/;

function validDate(text) {
  const match = DECISION_DATE.exec(text);
  if (!match) return false;
  const date = new Date(Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3])));
  return date.getUTCFullYear() === Number(match[1]) && date.getUTCMonth() === Number(match[2]) - 1 && date.getUTCDate() === Number(match[3]);
}

// A decision line `date · who · text`, split on the first two " · ", or null.
export function parseDecisionLine(text) {
  const one = text.indexOf(' · ');
  const two = one === -1 ? -1 : text.indexOf(' · ', one + 3);
  if (two === -1) return null;
  const date = text.slice(0, one).trim();
  const who = text.slice(one + 3, two).trim();
  const what = text.slice(two + 3).trim();
  if (!validDate(date) || !who || !what) return null;
  return { date, who, text: what };
}

/**
 * Every top-level list item of DECISIONS.md, outside code fences:
 * { days: [{ date, items: [{ who, text }] }] newest first, other: [text], total }.
 * Items that are not `date · who · text` go to other, in file order.
 */
export function parseDecisions(text) {
  const lines = (text ?? '').split('\n');
  const byDate = new Map();
  const other = [];
  let total = 0;
  let fence = null;
  for (let i = 0; i < lines.length; i += 1) {
    const marker = FENCE.exec(lines[i]);
    if (marker) {
      if (fence === null) fence = marker[1];
      else if (marker[1] === fence) fence = null;
      continue;
    }
    if (fence !== null) continue;
    const match = LIST_ITEM.exec(lines[i]);
    if (!match || indentOf(lines[i]) > 3) continue;
    const item = listItemAt(lines, i, lines[i].slice(match[0].length));
    i = item.end - 1;
    total += 1;
    const parsed = parseDecisionLine(item.paragraph);
    if (!parsed) {
      other.push([item.paragraph, item.nested].filter(Boolean).join('\n\n'));
      continue;
    }
    if (!byDate.has(parsed.date)) byDate.set(parsed.date, []);
    byDate.get(parsed.date).push({ who: parsed.who, text: parsed.text });
  }
  const days = [...byDate.entries()].sort(([a], [b]) => (a < b ? 1 : a > b ? -1 : 0)).map(([date, items]) => ({ date, items }));
  return { days, other, total };
}

// ---- product ----

export const WIDE_SECTIONS = Object.freeze(['who-it-is-for', 'the-problem', 'rules-that-hold-everywhere', 'the-loop']);

function firstParagraph(text) {
  const lines = (text ?? '').split('\n');
  let fence = null;
  for (let i = 0; i < lines.length; i += 1) {
    const marker = FENCE.exec(lines[i]);
    if (marker) {
      fence = fence === null ? marker[1] : marker[1] === fence ? null : fence;
      continue;
    }
    if (fence !== null || blank(lines[i]) || BLOCK_START.test(lines[i]) || LIST_ITEM.test(lines[i]) || indentOf(lines[i]) >= 4) continue;
    const out = [];
    for (let k = i; k < lines.length && !blank(lines[k]) && !BLOCK_START.test(lines[k]); k += 1) out.push(lines[k].trim());
    return out.join('\n');
  }
  return '';
}

/**
 * PRODUCT.md as the page lays it out: the hero is the section headed "In one
 * sentence" (else the file's first paragraph); every other `## ` section is a
 * tile, wide for the sections in WIDE_SECTIONS.
 */
export function productLayout(text) {
  const { sections } = splitSections(text ?? '');
  const heroSection = sections.find((section) => section.key === 'in-one-sentence');
  const bodyOf = (section) => section.text.split('\n').slice(1).join('\n').trim();
  return {
    hero: heroSection ? { key: heroSection.key, text: bodyOf(heroSection) } : { key: null, text: firstParagraph(text) },
    tiles: sections
      .filter((section) => section !== heroSection)
      .map((section) => ({ key: section.key, heading: section.heading, text: bodyOf(section), wide: WIDE_SECTIONS.includes(section.key) })),
  };
}

// ---- git ----

const HEX = /^[0-9a-f]{7,64}$/;

function git(repoRoot, args) {
  const env = { ...process.env, GIT_OPTIONAL_LOCKS: '0' };
  for (const name of ['GIT_DIR', 'GIT_WORK_TREE', 'GIT_INDEX_FILE', 'GIT_COMMON_DIR']) delete env[name];
  try {
    return execFileSync('git', ['-C', repoRoot, ...args], {
      encoding: 'utf8',
      timeout: 10_000,
      maxBuffer: 16 * 1024 * 1024,
      stdio: ['ignore', 'pipe', 'ignore'],
      env,
    });
  } catch {
    return null;
  }
}

export function headCommit(repoRoot) {
  const out = git(repoRoot, ['rev-parse', '--verify', 'HEAD']);
  const commit = out?.trim();
  return commit && HEX.test(commit) ? commit : null;
}

export function hasCommit(repoRoot, commit) {
  return HEX.test(commit) && git(repoRoot, ['cat-file', '-e', `${commit}^{commit}`]) !== null;
}

function fileAtCommit(repoRoot, commit, fileName) {
  const out = git(repoRoot, ['show', `${commit}:${DOC_FOLDER}/${fileName}`]);
  return out === null ? null : out.replace(/\r\n/g, '\n');
}

// ---- line diff ----

// [{ op: ' ' | '-' | '+', line }] using a longest-common-subsequence table.
export function lineDiff(before, after) {
  const a = before === '' ? [] : before.split('\n');
  const b = after === '' ? [] : after.split('\n');
  if (a.length * b.length > 4_000_000) {
    return [...a.map((line) => ({ op: '-', line })), ...b.map((line) => ({ op: '+', line }))];
  }
  const table = Array.from({ length: a.length + 1 }, () => new Uint32Array(b.length + 1));
  for (let i = a.length - 1; i >= 0; i -= 1) {
    for (let j = b.length - 1; j >= 0; j -= 1) {
      table[i][j] = a[i] === b[j] ? table[i + 1][j + 1] + 1 : Math.max(table[i + 1][j], table[i][j + 1]);
    }
  }
  const out = [];
  let i = 0;
  let j = 0;
  while (i < a.length && j < b.length) {
    if (a[i] === b[j]) {
      out.push({ op: ' ', line: a[i] });
      i += 1;
      j += 1;
    } else if (table[i + 1][j] >= table[i][j + 1]) {
      out.push({ op: '-', line: a[i] });
      i += 1;
    } else {
      out.push({ op: '+', line: b[j] });
      j += 1;
    }
  }
  for (; i < a.length; i += 1) out.push({ op: '-', line: a[i] });
  for (; j < b.length; j += 1) out.push({ op: '+', line: b[j] });
  return out;
}

// ---- changes since the recorded commit ----

function sectionMap(text) {
  const { preamble, sections } = splitSections(text ?? '');
  const map = new Map();
  if (preamble.trim()) map.set('', { heading: '', text: preamble });
  for (const section of sections) map.set(section.key, section);
  return map;
}

/**
 * Compares each product document with its text at `seenCommit`.
 * state: 'nogit' (no readable commit in the checkout), 'never' (no commit
 * recorded), 'unknown' (the recorded commit is gone), 'tracked'. Only
 * 'tracked' has changes: without a commit to compare with, nothing is
 * marked as new.
 * changes: [{ file, key, heading, status: 'changed' | 'new' | 'removed', before, after }]
 * Uncommitted edits count as changed because the comparison uses the working file.
 */
export function docChanges(repoRoot, seenCommit, docs) {
  const head = headCommit(repoRoot);
  let state = 'tracked';
  if (!head) state = 'nogit';
  else if (!seenCommit) state = 'never';
  else if (!hasCommit(repoRoot, seenCommit)) state = 'unknown';
  const changes = [];
  if (state !== 'tracked') return { state, head, changes };
  for (const [name, file] of Object.entries(DOC_FILES)) {
    const now = sectionMap(docs[name]);
    const old = sectionMap(fileAtCommit(repoRoot, seenCommit, file));
    for (const [key, section] of now) {
      const before = old.get(key);
      if (!before) changes.push({ file, key, heading: section.heading, status: 'new', before: '', after: section.text });
      else if (before.text !== section.text) {
        changes.push({ file, key, heading: section.heading, status: 'changed', before: before.text, after: section.text });
      }
    }
    for (const [key, section] of old) {
      if (!now.has(key)) changes.push({ file, key, heading: section.heading, status: 'removed', before: section.text, after: '' });
    }
  }
  return { state, head, changes };
}

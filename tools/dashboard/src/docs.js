// The product documents (docs/product/*.md): read live from the checkout,
// split into sections, parsed for journeys, and compared with the commit the
// owner last marked as seen. Nothing here writes to the checkout.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

export const DOC_FOLDER = 'docs/product';
export const DOC_FILES = Object.freeze({
  product: 'PRODUCT.md',
  journeys: 'JOURNEYS.md',
  decisions: 'DECISIONS.md',
});

export const STEP_STATUSES = Object.freeze(['built', 'partial', 'missing', 'untested']);
const STEP = /^(\s*[-*+]\s+)\[(built|partial|missing|untested)\](?=\s|$)\s*/i;
const FENCE = /^\s{0,3}(```|~~~)/;

export function slugify(text) {
  return String(text)
    .normalize('NFKD')
    .replace(/[̀-ͯ]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
}

// Reads one of the three known documents. A missing file, a missing folder, or
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
  const seen = new Map();
  return {
    preamble: preamble.join('\n').replace(/\s+$/, ''),
    sections: sections.map((section) => {
      const base = slugify(section.heading) || 'section';
      const count = (seen.get(base) ?? 0) + 1;
      seen.set(base, count);
      return {
        heading: section.heading,
        key: count === 1 ? base : `${base}-${count}`,
        text: section.lines.join('\n').replace(/\s+$/, ''),
      };
    }),
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
 * A step is `- [status] Title. Detail…`: the title is the text up to the
 * first ". " (or the whole first paragraph, without a final period); the
 * detail is the rest of that paragraph and anything nested under the item.
 */
export function splitStep(paragraph, nested = '') {
  const text = paragraph.trim();
  const end = sentenceEnd(text);
  const title = end === -1 ? text.replace(/\.$/, '') : text.slice(0, end);
  const rest = end === -1 ? '' : text.slice(end + 2).trim();
  return { title, detail: [rest, nested].filter(Boolean).join('\n\n') };
}

/**
 * Journeys: each `## ` heading is one. An `<!-- id: slug -->` line right under
 * the heading gives its id; otherwise the id is the slugified heading. A
 * leading "N. " in the heading is the journey's number (else its position).
 * The first paragraph under the heading is its promise. Steps are list items
 * that start with [built], [partial], [missing] or [untested] (exists in the
 * docs or code, not yet driven as a user); any other [word] stays plain text.
 * Everything else under the journey is its prose.
 */
export function parseJourneys(text) {
  const { preamble, sections } = splitSections(text);
  const used = new Set();
  const journeys = sections.map((section, position) => {
    const lines = section.text.split('\n').slice(1);
    let id = null;
    const first = lines.findIndex((line) => line.trim() !== '');
    if (first !== -1) {
      const match = /^<!--\s*id:\s*([A-Za-z0-9][A-Za-z0-9_-]*)\s*-->\s*$/.exec(lines[first]);
      if (match) {
        id = match[1].toLowerCase();
        lines.splice(first, 1);
      }
    }
    const base = id || slugify(section.heading) || 'journey';
    let unique = base;
    for (let n = 2; used.has(unique); n += 1) unique = `${base}-${n}`;
    used.add(unique);
    const body = lines.join('\n').replace(/^\s*\n/, '').replace(/\s+$/, '');
    const numbered = /^(\d{1,3})\.\s+(\S.*)$/.exec(section.heading);

    const bodyLines = body.split('\n');
    const taken = new Array(bodyLines.length).fill(false);
    let promise = '';
    const opening = bodyLines.findIndex((line) => !blank(line));
    if (opening !== -1 && !LIST_ITEM.test(bodyLines[opening]) && !BLOCK_START.test(bodyLines[opening]) && indentOf(bodyLines[opening]) < 4) {
      let end = opening;
      while (end < bodyLines.length && !blank(bodyLines[end]) && !(end > opening && (LIST_ITEM.test(bodyLines[end]) || BLOCK_START.test(bodyLines[end])))) end += 1;
      promise = bodyLines.slice(opening, end).map((line) => line.trim()).join(' ');
      for (let k = opening; k < end; k += 1) taken[k] = true;
    }
    const steps = [];
    let fence = null;
    for (let i = 0; i < bodyLines.length; i += 1) {
      const line = bodyLines[i];
      const marker = FENCE.exec(line);
      if (marker) {
        if (fence === null) fence = marker[1];
        else if (marker[1] === fence) fence = null;
        continue;
      }
      if (fence !== null || taken[i]) continue;
      const match = STEP.exec(line);
      if (!match) continue;
      const item = listItemAt(bodyLines, i, line.slice(match[0].length));
      steps.push({ n: steps.length + 1, status: match[2].toLowerCase(), ...splitStep(item.paragraph, item.nested) });
      for (let k = i; k < item.end; k += 1) taken[k] = true;
      i = item.end - 1;
    }
    const prose = bodyLines
      .filter((_, index) => !taken[index])
      .join('\n')
      .replace(/\n{3,}/g, '\n\n')
      .trim();
    const counts = Object.fromEntries(STEP_STATUSES.map((status) => [status, 0]));
    for (const step of steps) counts[step.status] += 1;
    return {
      id: unique,
      number: numbered ? Number(numbered[1]) : position + 1,
      title: numbered ? numbered[2] : section.heading,
      heading: section.heading,
      key: section.key,
      promise,
      body,
      steps,
      prose,
      counts,
    };
  });
  return { preamble, journeys };
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

// Top-level list entries of a Markdown body: one decision each.
export function countListItems(tokens) {
  return tokens.filter((token) => token.type === 'list').reduce((total, list) => total + list.items.length, 0);
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
 * recorded), 'unknown' (the recorded commit is gone), 'tracked'.
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
  if (state === 'nogit') return { state, head, changes };
  for (const [name, file] of Object.entries(DOC_FILES)) {
    const now = sectionMap(docs[name]);
    const old = state === 'tracked' ? sectionMap(fileAtCommit(repoRoot, seenCommit, file)) : new Map();
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

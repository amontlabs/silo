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

// Journeys: each `## ` heading is one. An `<!-- id: slug -->` line right under
// the heading gives its id; otherwise the id is the slugified heading. Steps
// are list items that start with [built], [partial], [missing] or [untested]
// (exists in the docs or code, not yet driven as a user). Any other [word]
// stays plain text.
export function parseJourneys(text) {
  const { preamble, sections } = splitSections(text);
  const used = new Set();
  const journeys = sections.map((section) => {
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
    const steps = [];
    eachLine(body, (line, inFence) => {
      const match = !inFence && STEP.exec(line);
      if (match) steps.push({ status: match[2].toLowerCase(), text: line.slice(match[0].length).trim() });
    });
    const counts = Object.fromEntries(STEP_STATUSES.map((status) => [status, 0]));
    for (const step of steps) counts[step.status] += 1;
    return { id: unique, title: section.heading, key: section.key, body, steps, counts };
  });
  return { preamble, journeys };
}

// Replaces the status markers of step lines with chip markup (documents render as-is, so raw HTML is fine).
export function chipSteps(markdown) {
  return eachLine(markdown, (line, inFence) =>
    inFence ? line : line.replace(STEP, (_, lead, status) => `${lead}<span class="chip step-${status.toLowerCase()}">${status.toLowerCase()}</span> `),
  ).join('\n');
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

function hasCommit(repoRoot, commit) {
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

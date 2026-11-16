// Private notes: the owner's Markdown files under $SILO_DASHBOARD_HOME/private/,
// kept out of the public repository. Read live on every request, never written.
// A file or folder whose real path leaves the private folder (a symbolic link
// pointing elsewhere) is skipped.
import fs from 'node:fs';
import path from 'node:path';

export const PRIVATE_FOLDER = 'private';
const MAX_NOTE_BYTES = 2 * 1024 * 1024;
const MAX_NOTES = 500;
const MAX_DEPTH = 8;

const byName = (a, b) => (a < b ? -1 : a > b ? 1 : 0);

function inside(root, file) {
  const relative = path.relative(root, file);
  return relative !== '' && !relative.startsWith('..') && !path.isAbsolute(relative);
}

/**
 * [{ folder, notes: [{ path, name, text }] }], folders sorted by path (the top
 * level, '', first), notes sorted by file name. `path` is relative to the
 * private folder, with forward slashes. A missing or empty folder gives [].
 */
export function readPrivateNotes(home) {
  let root;
  try {
    root = fs.realpathSync(path.join(home, PRIVATE_FOLDER));
    if (!fs.statSync(root).isDirectory()) return [];
  } catch {
    return [];
  }
  const groups = new Map();
  const visited = new Set([root]);
  let count = 0;

  const walk = (dir, relative, depth) => {
    let entries;
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    entries.sort((a, b) => byName(a.name, b.name));
    for (const entry of entries) {
      if (count >= MAX_NOTES) return;
      if (entry.name.startsWith('.')) continue;
      let real;
      let stat;
      try {
        real = fs.realpathSync(path.join(dir, entry.name));
        stat = fs.statSync(real);
      } catch {
        continue;
      }
      if (!inside(root, real)) continue;
      const rel = relative ? `${relative}/${entry.name}` : entry.name;
      if (stat.isDirectory()) {
        if (depth < MAX_DEPTH && !visited.has(real)) {
          visited.add(real);
          walk(real, rel, depth + 1);
        }
      } else if (stat.isFile() && /\.md$/i.test(entry.name) && stat.size <= MAX_NOTE_BYTES) {
        let text;
        try {
          text = fs.readFileSync(real, 'utf8').replace(/\r\n/g, '\n');
        } catch {
          continue;
        }
        if (!groups.has(relative)) groups.set(relative, []);
        groups.get(relative).push({ path: rel, name: entry.name, text });
        count += 1;
      }
    }
  };
  walk(root, '', 0);

  return [...groups.entries()]
    .sort(([a], [b]) => byName(a, b))
    .map(([folder, notes]) => ({ folder, notes: notes.sort((a, b) => byName(a.name, b.name)) }));
}

const plainText = (markdown) =>
  markdown
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/[`*_~]+/g, '')
    .replace(/\s+/g, ' ')
    .trim();

/**
 * What a note's tile shows: { title, summary }. The title is the note's first
 * `# ` heading, else its file name without .md; the summary is its first
 * paragraph as plain text, cut at `max` characters.
 */
export function noteSummary(note, max = 140) {
  const lines = note.text.split('\n');
  let title = null;
  let summary = '';
  let fence = false;
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    if (/^\s{0,3}(```|~~~)/.test(line)) {
      fence = !fence;
      continue;
    }
    if (fence) continue;
    const heading = /^# +(.+?)\s*#*\s*$/.exec(line);
    if (heading && title === null) {
      title = plainText(heading[1]);
      continue;
    }
    if (!summary && line.trim() && !/^\s{0,3}(#|[-*+]\s|\d+[.)]\s|>|\||<!--)/.test(line)) {
      const out = [];
      for (let k = i; k < lines.length && lines[k].trim() && !/^\s{0,3}#/.test(lines[k]); k += 1) out.push(lines[k]);
      summary = plainText(out.join(' '));
      if (title !== null) break;
    }
  }
  if (summary.length > max) summary = `${summary.slice(0, max - 1).trimEnd()}…`;
  return { title: title || note.name.replace(/\.md$/i, ''), summary };
}

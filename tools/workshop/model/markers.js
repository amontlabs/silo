// The marker the bot wrote on an entry's issue, and its reader.
//   <!-- silo-entry: <id> -->         last line of an entry's issue body

import { ID_PATTERN } from './names.js';

const ENTRY = /<!-- silo-entry: ([a-z0-9][a-z0-9-]{0,62}) -->/;

const normalized = (text) => (text ?? '').replace(/\r\n/g, '\n').trim();

/** @param {string} id */
export function entryMarker(id) {
  if (!ID_PATTERN.test(id)) throw new Error(`not a feature id: ${id}`);
  return `<!-- silo-entry: ${id} -->`;
}

/** @param {string | null | undefined} body @returns {string | null} the id of the entry marker ending the body. */
export function readEntryMarker(body) {
  const lines = normalized(body).split('\n');
  const match = ENTRY.exec(lines[lines.length - 1]);
  return match && lines[lines.length - 1].trim() === match[0] ? match[1] : null;
}

/**
 * The issues that name each entry, lowest number first. The lowest is the entry's thread; the others are
 * ignored, and `workshop doctor` reports them.
 * @param {Iterable<{ number: number, entry: string | null }>} issues @returns {Map<string, number[]>}
 */
export function issuesByEntry(issues) {
  /** @type {Map<string, number[]>} */
  const byEntry = new Map();
  for (const { number, entry } of issues) {
    if (!entry) continue;
    const numbers = byEntry.get(entry) ?? [];
    if (!numbers.includes(number)) numbers.push(number);
    byEntry.set(entry, numbers);
  }
  for (const numbers of byEntry.values()) numbers.sort((a, b) => a - b);
  return byEntry;
}

/** @param {string | null | undefined} body @returns {string} the body without the entry marker. */
export function withoutMarkers(body) {
  return normalized(body)
    .split('\n')
    .filter((line) => !ENTRY.test(line))
    .join('\n')
    .trim();
}

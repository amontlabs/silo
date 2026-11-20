// The plan's entries as the server knows them: a file made at build time from plan/ and shipped beside
// the server. For each entry: its id, its kind, its title (a decision's `asks`, a feature's `name`),
// its part (null for the whole product) and `suggestedIn`, the suggestion it was written from. The
// server never trusts an id or a title the page sends: it looks the id up here.
import { readFileSync } from 'node:fs';
import { ID_PATTERN } from '../../tools/workshop/model/index.js';

export const ENTRIES_FILE = 'plan-entries.json';
const KINDS = ['decision', 'feature'];

/** The entries of a list read from the file, by id; throws on anything that is not one. */
export function readPlanEntries(list) {
  if (!Array.isArray(list)) throw new Error('not a list of entries');
  const entries = new Map();
  for (const item of list) {
    const { id, kind, title, part, suggestedIn } = item ?? {};
    if (typeof id !== 'string' || !ID_PATTERN.test(id) || entries.has(id)) throw new Error('an entry id is not valid');
    if (!KINDS.includes(kind)) throw new Error(`the entry ${id} has no kind`);
    if (typeof title !== 'string' || !title.trim()) throw new Error(`the entry ${id} has no title`);
    if (part !== null && (typeof part !== 'string' || !ID_PATTERN.test(part))) throw new Error(`the entry ${id} has no valid part`);
    if (suggestedIn !== null && !(Number.isSafeInteger(suggestedIn) && suggestedIn > 0)) throw new Error(`the entry ${id} has no valid suggestedIn`);
    entries.set(id, Object.freeze({ id, kind, title, part, suggestedIn }));
  }
  return entries;
}

/** The file shipped beside this module, or `{ problem }` when it is missing or not valid. */
export function loadPlanEntries(url = new URL(`./${ENTRIES_FILE}`, import.meta.url)) {
  try {
    return { entries: readPlanEntries(JSON.parse(readFileSync(url, 'utf8'))) };
  } catch (error) {
    return { entries: new Map(), problem: error?.code === 'ENOENT' ? 'missing' : 'invalid' };
  }
}

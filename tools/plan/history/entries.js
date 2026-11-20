// The history of each entry of the plan, from the steps that changed it. Pure: no `node:` import.

import { entryPath } from './paths.js';

/** What a change can name, in the order they are always listed. Nothing else is ever put in `what`. */
export const WHAT = Object.freeze([
  'added', 'moved', 'text', 'sets', 'needs', 'keeps', 'surfaces', 'suggest', 'notes', 'taken', 'dropped', 'cleared', 'replaced', 'built',
]);

/** The keys that give an entry's title and body, by kind. */
const TEXT_KEYS = { decision: ['asks', 'answer'], feature: ['name', 'what'] };

/** A value as text with the keys of every object sorted: two values are the same when their texts are. */
function canon(value) {
  if (value === undefined) return 'undefined';
  if (Array.isArray(value)) return `[${value.map(canon).join(',')}]`;
  if (value !== null && typeof value === 'object') return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canon(value[key])}`).join(',')}}`;
  return JSON.stringify(value);
}

const same = (a, b) => canon(a) === canon(b);

/** @param {string | null | undefined} text @returns {Record<string, any> | null} the parsed JSON object, or null when it is not one. */
function parse(text) {
  if (typeof text !== 'string') return null;
  try {
    const value = JSON.parse(text);
    return value !== null && typeof value === 'object' && !Array.isArray(value) ? value : null;
  } catch {
    return null;
  }
}

/** @param {Record<string, any>} value @returns {'take' | 'drop' | null} */
const verdictOf = (value) => (value.verdict === 'take' || value.verdict === 'drop' ? value.verdict : null);

/**
 * @typedef {{ path: string, part: string | null, kind: 'decision' | 'feature', value: Record<string, any>, notes: string | null }} Snapshot
 * The entry as one step left it: where its file is, what it holds, and its notes.
 */

/**
 * The names a step gives to an entry that was there before it.
 * @param {Snapshot} before @param {Snapshot} after @returns {{ what: string[], text?: object, by?: unknown }}
 */
function compare(before, after) {
  const was = before.value;
  const now = after.value;
  const differ = (...keys) => keys.some((key) => !same(was[key], now[key]));
  const names = new Set();
  const [titleKey, bodyKey] = TEXT_KEYS[after.kind];
  if (before.part !== after.part) names.add('moved');
  if (differ(titleKey, bodyKey)) names.add('text');
  const verdictBefore = verdictOf(was);
  const verdictAfter = verdictOf(now);
  if (differ('sets', 'removes', 'replaces') && verdictBefore === null && verdictAfter === null) names.add('sets');
  for (const key of ['needs', 'keeps', 'surfaces', 'suggest']) if (differ(key)) names.add(key);
  if (before.notes !== after.notes) names.add('notes');
  if (verdictAfter === 'take' && verdictBefore !== 'take') names.add('taken');
  if (verdictAfter === 'drop' && verdictBefore !== 'drop') names.add('dropped');
  if (verdictBefore !== null && verdictAfter === null) names.add('cleared');
  if (was.replaced_by === undefined && now.replaced_by !== undefined) names.add('replaced');
  if (was.built === undefined && now.built !== undefined) names.add('built');
  const what = WHAT.filter((name) => names.has(name));
  const extra = {};
  if (names.has('text')) {
    const side = (value) => ({ title: value[titleKey] ?? null, body: value[bodyKey] ?? null });
    extra.text = { before: side(was), after: side(now) };
  }
  if (names.has('replaced')) extra.by = now.replaced_by;
  return { what, ...extra };
}

/**
 * The history of each entry: what each step that changed it did, newest first.
 *
 * `steps` are the commits that changed `plan/`, oldest first. `files` maps a path relative to
 * `plan/` to the file's text before and after the step, null when it did not exist. An entry's
 * file is `decisions/<id>.json`, `<part>/decisions/<id>.json` or `<part>/features/<id>.json`, and
 * its notes the `<id>.md` beside it; other paths are ignored. A step where the entry's file is
 * not a JSON object is skipped for that entry. A step that deletes the file gives no change, and
 * the file coming back is `added` again. A step that changes none of the names of `WHAT` gives no change.
 *
 * @param {{ commit: string, date: string, files: Record<string, { before: string | null, after: string | null }> }[]} steps
 * @returns {Map<string, { commit: string, date: string, what: string[], text?: object, by?: unknown }[]>}
 */
export function entryHistory(steps) {
  /** @type {Map<string, string | null>} the text of each entry file and notes file, by path, as of the steps seen */
  const tree = new Map();
  /** @type {Set<string>} */
  const seen = new Set();
  /** @type {Map<string, Set<string>>} the paths of the entry files that exist, by id */
  const live = new Map();
  /** @type {Map<string, Snapshot | null>} the last version of each entry that parsed; null when it is gone */
  const last = new Map();
  /** @type {Map<string, any[]>} */
  const history = new Map();

  const livePaths = (id) => {
    let set = live.get(id);
    if (!set) live.set(id, (set = new Set()));
    return set;
  };
  /** Sets the text of a path, keeping `live` in step. */
  const put = (path, info, text) => {
    if (text === null) tree.delete(path);
    else tree.set(path, text);
    if (!info.notes) {
      if (text === null) livePaths(info.id).delete(path);
      else livePaths(info.id).add(path);
    }
  };
  /** The entry as the tree holds it now, or undefined when its file is absent or does not parse. null: absent. */
  const snapshot = (id, tracked) => {
    const paths = [...livePaths(id)].sort();
    if (!paths.length) return null;
    const path = tracked && paths.includes(tracked) ? tracked : paths[0];
    const value = parse(tree.get(path));
    if (!value) return undefined;
    const info = entryPath(path);
    const notes = tree.get(`${info.dir}${id}.md`) ?? null;
    return { path, part: info.part, kind: info.kind, value, notes };
  };

  for (const { commit, date, files } of steps ?? []) {
    /** @type {Map<string, { id: string, part: string | null, kind: string, notes: boolean, dir: string }>} */
    const changed = new Map();
    for (const [path, texts] of Object.entries(files ?? {})) {
      const info = entryPath(path);
      if (info) changed.set(path, { ...info });
      // A path not seen before starts as the text the step says it had.
      if (info && !seen.has(path)) {
        seen.add(path);
        if (typeof texts?.before === 'string') put(path, info, texts.before);
      }
    }
    const ids = new Set([...changed.values()].map((info) => info.id));
    for (const id of ids) if (!last.has(id)) last.set(id, snapshot(id) ?? null);
    for (const [path, info] of changed) {
      const after = files[path]?.after;
      put(path, info, typeof after === 'string' ? after : null);
    }
    for (const id of ids) {
      const before = last.get(id) ?? null;
      const now = snapshot(id, before?.path);
      if (now === undefined) continue;
      last.set(id, now);
      if (now === null) continue;
      const change = before === null ? { what: ['added'] } : compare(before, now);
      if (!change.what.length) continue;
      const list = history.get(id) ?? [];
      list.unshift({ commit, date, ...change });
      history.set(id, list);
    }
  }
  return history;
}

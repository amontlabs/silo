// `take` and `drop`: the owner's verdict on an entry, as pure functions. Files in (the files of
// `plan/`, by path relative to it), changed files out. The command wraps them with git; the
// Workshop can call them as they are.

import { checkFiles } from '../check/index.js';
import { canonicalDecision, canonicalFeature, canonicalPrinciple, canonicalProperty, canonicalText, canonicalWord } from '../model/canonical.js';
import { listKind, wordId } from '../model/names.js';
import { hasSets } from '../model/status.js';
import { parsePlan } from '../read/index.js';

/** @typedef {{ file: string, field: string, message: string, fix: string }} Problem */
/**
 * What a verdict did. `changed` holds the new text of each file that changed, by its path
 * relative to `plan/`; it is empty unless `status` is `done`.
 * @typedef {{ status: 'done' | 'already' | 'refused', changed: Map<string, string>, message: string, errors?: Problem[] }} Outcome
 */
/** @typedef {{ exists?: (path: string) => boolean }} Options */

const isObject = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);
const refused = (message, errors) => ({ status: 'refused', changed: new Map(), message, ...(errors ? { errors } : {}) });

/** The path of an entry's file, relative to `plan/`. @param {{ id: string, kind: string, part: string | null }} entry */
export const entryFile = (entry) => `${entry.part ? `${entry.part}/` : ''}${entry.kind === 'decision' ? 'decisions' : 'features'}/${entry.id}.json`;

/** The file of a list a `sets` item names (`words`, `principles`, `properties`, `<part>/properties`). @param {string} list */
const listFile = (list) => `${list}.json`;

const LIST_FILE = /^(?:words|principles|(?:[^/]+\/)?properties)\.json$/;
const CANONICAL_SENTENCE = { word: canonicalWord, principle: canonicalPrinciple, property: canonicalProperty };

/** @param {string} name a list's file @returns {'word' | 'principle' | 'property'} */
const fileKind = (name) => (name === 'words.json' ? 'word' : name === 'principles.json' ? 'principle' : 'property');

/** A sentence's id: a word's comes from the word itself. @param {'word' | 'principle' | 'property'} kind @param {any} sentence */
const sentenceId = (kind, sentence) => (kind === 'word' ? wordId(sentence.word) : sentence.id);

/**
 * The plan as it parsed, the entry `id` and its file, or a refusal: the plan does not read, fails
 * the check, or has no entry by that id.
 * @param {Map<string, string>} files @param {string} id @param {Options} options
 */
function load(files, id, options) {
  const { plan, problems, json } = parsePlan(files);
  if (problems.length) return { refusal: refused('the plan does not read', problems) };
  const { errors } = checkFiles(files, options);
  if (errors.length) return { refusal: refused(`the plan fails the check (${errors.length} error${errors.length === 1 ? '' : 's'}); plan check lists them`, errors) };
  if (!Object.hasOwn(plan.entries, id)) return { refusal: refused(`no decision or feature has the id "${id}"`) };
  const entry = plan.entries[id];
  return { plan, json, entry, file: entryFile(entry) };
}

/**
 * The changed files, after checking the plan they make. A result that fails the check is
 * refused and nothing is written.
 * @param {Map<string, string>} files @param {Map<string, string>} changed @param {Options} options @param {string} message
 * @returns {Outcome}
 */
function finish(files, changed, options, message) {
  for (const [name, text] of changed) if (files.get(name) === text) changed.delete(name);
  const after = new Map(files);
  for (const [name, text] of changed) after.set(name, text);
  const { errors } = checkFiles(after, options);
  if (errors.length) return refused(`the plan would fail the check (${errors.length} error${errors.length === 1 ? '' : 's'}); nothing was written`, errors);
  return { status: 'done', changed, message };
}

/** @param {string} kind @param {any} value */
const entryText = (kind, value) => canonicalText(kind === 'decision' ? canonicalDecision(value) : canonicalFeature(value));

/**
 * The owner's yes. Puts each `sets` item in its list (the same id is replaced whole, in place; a
 * new id is appended), deletes each sentence `removes` names, takes those ids out of every other
 * entry's `adds`, and writes `adds` and `"verdict": "take"` in the entry in place of `sets` and
 * `removes`.
 * @param {Map<string, string>} files @param {string} id @param {Options} [options] @returns {Outcome}
 */
export function take(files, id, options = {}) {
  const loaded = load(files, id, options);
  if (loaded.refusal) return loaded.refusal;
  const { plan, json, entry, file } = loaded;
  if (entry.built) return refused(`${id} is built`);
  if (entry.verdict === 'take') return { status: 'already', changed: new Map(), message: `${id} is already taken` };
  if (entry.kind === 'decision') {
    if (entry.answer === undefined && !hasSets(entry)) return refused(`${id} has no answer and sets nothing: write its answer first`);
    const waiting = (entry.needs ?? []).filter((need) => plan.entries[need]?.kind === 'decision' && plan.entries[need].verdict !== 'take');
    if (waiting.length) return refused(`${id} needs ${waiting.join(', ')}, not taken yet`);
  }

  // Every list, as arrays of sentences, by file. A part's list that does not exist yet starts empty.
  /** @type {Map<string, any[]>} */
  const lists = new Map();
  for (const [name, value] of json) if (LIST_FILE.test(name) && Array.isArray(value)) lists.set(name, structuredClone(value));
  const locate = (sentence) => {
    for (const [name, list] of lists) {
      const index = list.findIndex((item) => isObject(item) && sentenceId(fileKind(name), item) === sentence);
      if (index !== -1) return { name, list, index };
    }
    return null;
  };

  const sets = Array.isArray(entry.sets) ? entry.sets : [];
  const removes = Array.isArray(entry.removes) ? entry.removes : [];
  const setIds = [];
  for (const item of sets) {
    const { in: list, ...sentence } = item;
    const kind = listKind(list);
    const name = listFile(list);
    const sid = sentenceId(kind, sentence);
    setIds.push(sid);
    const value = CANONICAL_SENTENCE[kind](sentence);
    const found = locate(sid);
    if (found && found.name === name) {
      found.list[found.index] = value;
      continue;
    }
    // The id lives in another list (a property moved to a part): it leaves that one.
    if (found) found.list.splice(found.index, 1);
    if (!lists.has(name)) lists.set(name, []);
    lists.get(name).push(value);
  }
  for (const sid of removes) {
    const found = locate(sid);
    if (found) found.list.splice(found.index, 1);
  }

  /** @type {Map<string, string>} */
  const changed = new Map();
  for (const [name, list] of lists) changed.set(name, canonicalText(list.map((item) => (isObject(item) ? CANONICAL_SENTENCE[fileKind(name)](item) : item))));

  // An id this entry brings or takes out no longer belongs to the entry that brought it before.
  const moved = new Set([...setIds, ...removes]);
  for (const other of Object.values(plan.entries)) {
    if (other.id === id || !Array.isArray(other.adds) || !other.adds.some((sid) => moved.has(sid))) continue;
    const otherFile = entryFile(other);
    const data = structuredClone(json.get(otherFile));
    data.adds = data.adds.filter((sid) => !moved.has(sid));
    if (!data.adds.length) delete data.adds;
    changed.set(otherFile, entryText(other.kind, data));
  }

  const data = structuredClone(json.get(file));
  delete data.sets;
  delete data.removes;
  if (setIds.length) data.adds = setIds;
  else delete data.adds;
  data.verdict = 'take';
  changed.set(file, entryText(entry.kind, data));
  return finish(files, changed, options, `took ${id}`);
}

/**
 * The owner's no: `"verdict": "drop"`, with `sets` and `removes` kept. Refused on a taken entry
 * that has `adds`: its sentences are in force.
 * @param {Map<string, string>} files @param {string} id @param {Options} [options] @returns {Outcome}
 */
export function drop(files, id, options = {}) {
  const loaded = load(files, id, options);
  if (loaded.refusal) return loaded.refusal;
  const { json, entry, file } = loaded;
  if (entry.verdict === 'drop') return { status: 'already', changed: new Map(), message: `${id} is already dropped` };
  if (entry.built) return refused(`${id} is built`);
  if (entry.verdict === 'take' && Array.isArray(entry.adds) && entry.adds.length) {
    return refused(`${id} is taken and brought ${entry.adds.join(', ')}: an entry that removes or replaces them comes first`);
  }
  const data = structuredClone(json.get(file));
  data.verdict = 'drop';
  return finish(files, new Map([[file, entryText(entry.kind, data)]]), options, `dropped ${id}`);
}

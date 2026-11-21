// `take` and `drop`: the owner's verdict on an entry, as pure functions. Files in (the files of
// `plan/`, by path relative to it), changed files out. The command wraps them with git; the
// Workshop can call them as they are.

import { checkFiles } from '../check/index.js';
import { canonicalDecision, canonicalFeature, canonicalPrinciple, canonicalProperty, canonicalText, canonicalWord } from '../model/canonical.js';
import { listKind, wordId } from '../model/names.js';
import { hasSets, replacedBy } from '../model/status.js';
import { parsePlan } from '../read/index.js';

/** @typedef {{ file: string, field: string, message: string, fix: string }} Problem */
/**
 * What a verdict did. `changed` holds the new text of each file that changed, by its path
 * relative to `plan/`; it is empty unless `status` is `done`. A verdict deletes no file.
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
const CANONICAL_STATEMENT = { word: canonicalWord, principle: canonicalPrinciple, property: canonicalProperty };

/** @param {string} name a list's file @returns {'word' | 'principle' | 'property'} */
const fileKind = (name) => (name === 'words.json' ? 'word' : name === 'principles.json' ? 'principle' : 'property');

/** A statement's id: a word's comes from the word itself. @param {'word' | 'principle' | 'property'} kind @param {any} statement */
const statementId = (kind, statement) => (kind === 'word' ? wordId(statement.word) : statement.id);

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
 * new id is appended), deletes each statement `removes` names, takes those ids out of every other
 * entry's `adds`, and writes `adds` and `"verdict": "take"` in the entry in place of `sets` and
 * `removes`; `replaces` stays. Each decision named in `replaces` stays too, with its file and its
 * notes, and gets `replaced_by` with this entry's id: it is replaced, and no longer in force. The
 * take is refused when a decision it names is not taken or already replaced, when one would keep
 * an `adds`, and when a taken decision it does not name would be left with neither an `answer` nor
 * an `adds`.
 * @param {Map<string, string>} files @param {string} id @param {Options} [options] @returns {Outcome}
 */
export function take(files, id, options = {}) {
  const loaded = load(files, id, options);
  if (loaded.refusal) return loaded.refusal;
  const { plan, json, entry, file } = loaded;
  if (entry.built) return refused(`${id} is built`);
  if (entry.verdict === 'take') {
    const by = replacedBy(entry);
    return { status: 'already', changed: new Map(), message: by ? `${id} is already taken, and replaced by ${by}` : `${id} is already taken` };
  }
  if (entry.kind === 'decision') {
    if (entry.answer === undefined && !hasSets(entry)) return refused(`${id} has no answer and sets nothing: write its answer first`);
    const waiting = (entry.needs ?? []).filter((need) => plan.entries[need]?.kind === 'decision' && plan.entries[need].verdict !== 'take');
    if (waiting.length) return refused(`${id} needs ${waiting.join(', ')}, not taken yet`);
  }
  // Each decision it replaces is taken, and replaced by no other decision yet.
  const replaces = entry.kind === 'decision' && Array.isArray(entry.replaces) ? entry.replaces : [];
  for (const target of replaces) {
    const other = Object.hasOwn(plan.entries, target) ? plan.entries[target] : undefined;
    if (other?.kind !== 'decision' || other.verdict !== 'take') return refused(`${id} replaces ${target}, which is not taken: edit ${target} instead, it has no verdict to replace`);
    const by = replacedBy(other);
    if (by) return refused(`${id} replaces ${target}, which ${by} already replaced: a decision is replaced by one decision only, so ${id} replaces ${by}, the later one, instead`);
  }

  // Every list, as arrays of statements, by file. A part's list that does not exist yet starts empty.
  /** @type {Map<string, any[]>} */
  const lists = new Map();
  for (const [name, value] of json) if (LIST_FILE.test(name) && Array.isArray(value)) lists.set(name, structuredClone(value));
  const locate = (statement) => {
    for (const [name, list] of lists) {
      const index = list.findIndex((item) => isObject(item) && statementId(fileKind(name), item) === statement);
      if (index !== -1) return { name, list, index };
    }
    return null;
  };

  const sets = Array.isArray(entry.sets) ? entry.sets : [];
  const removes = Array.isArray(entry.removes) ? entry.removes : [];
  const setIds = [];
  for (const item of sets) {
    const { in: list, ...statement } = item;
    const kind = listKind(list);
    const name = listFile(list);
    const sid = statementId(kind, statement);
    setIds.push(sid);
    const value = CANONICAL_STATEMENT[kind](statement);
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
  for (const [name, list] of lists) changed.set(name, canonicalText(list.map((item) => (isObject(item) ? CANONICAL_STATEMENT[fileKind(name)](item) : item))));

  // An id this entry brings or takes out no longer belongs to the entry that brought it before.
  const moved = new Set([...setIds, ...removes]);
  /** @type {string[]} the taken decisions it does not replace and that would be left with nothing in force */
  const emptied = [];
  for (const other of Object.values(plan.entries)) {
    if (other.id === id || !Array.isArray(other.adds) || !other.adds.some((sid) => moved.has(sid))) continue;
    const otherFile = entryFile(other);
    const data = structuredClone(json.get(otherFile));
    data.adds = data.adds.filter((sid) => !moved.has(sid));
    if (!data.adds.length) delete data.adds;
    if (other.kind === 'decision' && other.verdict === 'take' && data.answer === undefined && data.adds === undefined && !replaces.includes(other.id)) emptied.push(other.id);
    changed.set(otherFile, entryText(other.kind, data));
  }
  // A decision it replaces is no longer in force: none of its statements may stay.
  const kept = replaces.map((target) => {
    const name = entryFile(plan.entries[target]);
    const adds = changed.has(name) ? JSON.parse(/** @type {string} */ (changed.get(name))).adds : plan.entries[target].adds;
    return [target, Array.isArray(adds) ? adds : []];
  }).filter(([, adds]) => adds.length);
  if (kept.length) {
    return refused(`${id} replaces ${kept.map(([target]) => target).join(', ')}, which would still add ${kept.flatMap(([, adds]) => adds).join(', ')}: set them again or remove them in ${id}`);
  }
  // A taken decision whose every statement this take moves, and that has no answer, is replaced whole: the entry says so.
  if (emptied.length) {
    const fix = entry.kind === 'decision' ? `name ${emptied.length === 1 ? 'it' : 'them'} in the replaces of ${id}` : `a decision that replaces ${emptied.length === 1 ? 'it' : 'them'} comes first`;
    return refused(`${id} moves every statement of ${emptied.join(', ')}, which ${emptied.length === 1 ? 'has' : 'have'} no answer, so ${emptied.length === 1 ? 'it' : 'they'} would be left with nothing in force: ${fix}`);
  }

  // Each decision it replaces says so in its own file.
  for (const target of replaces) {
    const name = entryFile(plan.entries[target]);
    const replaced = changed.has(name) ? JSON.parse(/** @type {string} */ (changed.get(name))) : structuredClone(json.get(name));
    replaced.replaced_by = id;
    changed.set(name, entryText('decision', replaced));
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
 * The owner's no: `"verdict": "drop"`, with `sets`, `removes` and `replaces` kept. Only a taken
 * feature holds `started`, so a feature being built loses it in the same write, and the message
 * says so. Refused on a
 * replaced decision, on a taken entry that has `adds` (its statements are in force), and on a taken
 * decision that has `replaces` (the decisions it replaced would be in force again).
 * @param {Map<string, string>} files @param {string} id @param {Options} [options] @returns {Outcome}
 */
export function drop(files, id, options = {}) {
  const loaded = load(files, id, options);
  if (loaded.refusal) return loaded.refusal;
  const { plan, json, entry, file } = loaded;
  if (entry.verdict === 'drop') return { status: 'already', changed: new Map(), message: `${id} is already dropped` };
  if (entry.built) return refused(`${id} is built`);
  const by = replacedBy(entry);
  if (by) return refused(`${id} is replaced by ${by}: it is no longer in force, and has nothing to drop`);
  if (entry.verdict === 'take' && Array.isArray(entry.adds) && entry.adds.length) {
    return refused(`${id} is taken and brought ${entry.adds.join(', ')}: an entry that removes or replaces them comes first`);
  }
  if (entry.verdict === 'take' && Array.isArray(entry.replaces) && entry.replaces.length) {
    return refused(`${id} is taken and replaces ${entry.replaces.join(', ')}: a decision that replaces ${id} comes first`);
  }
  if (entry.verdict === 'take') {
    const waiting = Object.values(plan.entries).filter((other) => other.kind === 'decision' && other.verdict === undefined && Array.isArray(other.replaces) && other.replaces.includes(id)).map((other) => other.id);
    if (waiting.length) return refused(`${id} is named in the replaces of ${waiting.join(', ')}, not decided: take or drop ${waiting.length === 1 ? 'it' : 'them'} first, or remove ${id} from ${waiting.length === 1 ? 'its' : 'their'} replaces`);
  }
  const data = structuredClone(json.get(file));
  data.verdict = 'drop';
  const building = data.started !== undefined;
  delete data.started;
  return finish(files, new Map([[file, entryText(entry.kind, data)]]), options, building ? `dropped ${id}, which was being built: its started date is removed` : `dropped ${id}`);
}

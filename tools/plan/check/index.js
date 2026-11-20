// `plan check`: the checks of `plan/`, rules 1 to 11 of the migration contract.
//
//   checkFiles(files, { exists })  rules 1 to 10, on a tree of files: { errors, notes }
//   checkChange(files, baseFiles)  rule 11, against the merged plan: { errors }
//
// A problem, error or note, is { file, field, message, fix }. `field` is `-` for the whole file.

import {
  CHECKS, DECISION_KEYS, FEATURE_KEYS, ID_PATTERN, ORDER_KEYS, PART_KEYS, PRINCIPLE_KEYS, PROPERTY_KEYS, PROPERTY_KINDS,
  REQUIRED_KEYS, STAGE_KEYS, SUGGEST_KEYS, SURFACES_KEYS, VERDICTS, WORD_KEYS, isDate, listKind, wordId,
} from '../model/names.js';
import {
  canonicalDecision, canonicalFeature, canonicalOrder, canonicalPart, canonicalPrinciple, canonicalProperty, canonicalText, canonicalWord,
} from '../model/canonical.js';
import { isDone } from '../model/status.js';
import { isHidden } from '../read/index.js';

/** @typedef {{ file: string, field: string, message: string, fix: string }} Problem */

/** @param {Problem} p */
export const formatProblem = (p) => `${p.file}: ${p.field}: ${p.message}. Fix: ${p.fix}`;

const isObject = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);
const isText = (value) => typeof value === 'string' && value.trim() !== '';
const each = (fn) => (list) => (Array.isArray(list) ? list.map(fn) : list);
const at = (field, key) => (field ? `${field}.${key}` : key);
/** A value as a message shows it: a text as it is, anything else as JSON, so a malformed field never throws. */
const shown = (value) => (typeof value === 'string' ? value : JSON.stringify(value) ?? String(typeof value));

/**
 * Why a path of an `at` is not a repository path, or null: it must be relative, stay inside the
 * repository and name a file, so no empty, `.` or `..` part, no backslash and no drive letter.
 * @param {string} path
 */
function badPath(path) {
  if (path.startsWith('/') || /^[A-Za-z]:/.test(path) || path.includes('\\')) return 'is not relative to the top of the repository';
  if (path.split('/').some((part) => part === '' || part === '.' || part === '..')) return 'has an empty, "." or ".." part';
  return null;
}

/** The files at the root of `plan/` that are not entries. */
const ROOT_FILES = new Set(['README.md', 'PRODUCT.md', 'parts.json', 'order.json', 'words.json', 'principles.json', 'properties.json']);
const LISTS = ['parts.json', 'order.json', 'words.json', 'principles.json'];
/** `decisions/<id>.json|md` at the root, `<part>/decisions/…` or `<part>/features/…` in a part. */
const ENTRY_FILE = /^(?:([^/]+)\/)?(decisions|features)\/([^/]+)\.(json|md)$/;
const PART_FILE = /^([^/]+)\/(README\.md|properties\.json)$/;
const RESERVED_PARTS = new Set(['decisions', 'features']);

const STATEMENT_KEYS = { word: WORD_KEYS, principle: PRINCIPLE_KEYS, property: PROPERTY_KEYS };

/** The canonical form of a JSON file of the plan, by its path relative to `plan/`. */
function canonicalOf(name) {
  if (name === 'parts.json') return each(canonicalPart);
  if (name === 'order.json') return canonicalOrder;
  if (name === 'words.json') return each(canonicalWord);
  if (name === 'principles.json') return each(canonicalPrinciple);
  if (/^(?:[^/]+\/)?properties\.json$/.test(name)) return each(canonicalProperty);
  if (/(?:^|\/)decisions\/[^/]+\.json$/.test(name)) return canonicalDecision;
  return canonicalFeature;
}

/** An optional key that holds nothing: absent is the only way to say "none". */
const isEmpty = (value) => value === null || (typeof value === 'string' && value.trim() === '')
  || (Array.isArray(value) && value.length === 0) || (isObject(value) && Object.keys(value).length === 0);

/** A word or a phrase, matched on word boundaries, in any case, its spaces matching any run of spaces. */
function phrasePattern(phrase) {
  const body = phrase.trim().split(/\s+/).map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('\\s+');
  return new RegExp(`(?<![\\p{L}\\p{N}_])${body}(?![\\p{L}\\p{N}_])`, 'iu');
}

/** The id of a statement: a word's is derived from `word`, the others carry `id`. */
const statementId = (kind, statement) => (kind === 'word' ? (typeof statement.word === 'string' ? wordId(statement.word) : null) : statement.id);
const idField = (kind) => (kind === 'word' ? 'word' : 'id');

/**
 * Rules 1 to 10 on a tree of files.
 * @param {Map<string, string>} files the files of `plan/`, by path relative to it
 * @param {{ exists?: (path: string) => boolean }} [options] `exists` says whether a repository path exists (rule 9's file test)
 * @returns {{ errors: Problem[], notes: Problem[] }}
 */
export function checkFiles(files, { exists } = {}) {
  /** @type {Problem[]} */
  const errors = [];
  /** @type {Problem[]} */
  const notes = [];
  const add = (file, field, message, fix) => errors.push({ file, field, message, fix });
  const names = [...files.keys()].filter((name) => !isHidden(name)).sort();

  /** Reports keys that are not in `keys`. */
  function unknownKeys(file, field, object, keys) {
    for (const key of Object.keys(object)) if (!keys.includes(key)) add(file, at(field, key), 'unknown key', `remove it; the keys are ${keys.join(', ')}`);
  }
  /** Reports required keys that are missing or empty, and optional keys that hold nothing. */
  function present(file, field, object, required) {
    for (const key of required) if (!Object.hasOwn(object, key) || isEmpty(object[key])) add(file, at(field, key), 'missing or empty', `write ${key}`);
    for (const [key, value] of Object.entries(object)) {
      if (!required.includes(key) && isEmpty(value)) add(file, at(field, key), 'empty', 'leave an optional key out instead of leaving it empty');
    }
  }
  function textKey(file, field, object, key) {
    if (Object.hasOwn(object, key) && !isEmpty(object[key]) && typeof object[key] !== 'string') add(file, at(field, key), 'not a text', `write ${key} as a string`);
  }
  /** A list of distinct texts; returns its items that are texts. */
  function textList(file, field, value, what) {
    if (value === undefined || isEmpty(value)) return [];
    if (!Array.isArray(value)) {
      add(file, field, 'not a list', `write a list of ${what}`);
      return [];
    }
    const seen = new Set();
    value.forEach((item, index) => {
      if (!isText(item)) add(file, `${field}[${index}]`, 'not a text', `write ${what} as strings`);
      else if (seen.has(item)) add(file, `${field}[${index}]`, `"${item}" appears twice`, 'list it once');
      seen.add(item);
    });
    return value.filter(isText);
  }

  // Rule 2: UTF-8, valid JSON, canonical text. Every JSON file is parsed once here.
  /** @type {Map<string, any>} */
  const json = new Map();
  for (const name of names) {
    if (!name.endsWith('.json')) continue;
    const text = /** @type {string} */ (files.get(name));
    let value;
    try {
      value = JSON.parse(text);
    } catch (error) {
      add(`plan/${name}`, '-', `not JSON (${/** @type {Error} */ (error).message})`, 'write valid JSON');
      continue;
    }
    json.set(name, value);
    if (text !== canonicalText(canonicalOf(name)(structuredClone(value)))) {
      add(`plan/${name}`, '-', 'not in canonical form', 'write it as JSON.stringify(value, null, 2) with a final newline, keys in the order of plan/README.md');
    }
  }

  // parts.json first: the folder layout depends on it.
  const partsFile = 'plan/parts.json';
  /** @type {Set<string>} */
  const partIds = new Set();
  /** @type {Map<string, string>} the field of each part id in parts.json */
  const partFields = new Map();
  const parts = json.get('parts.json');
  if (parts !== undefined) {
    if (!Array.isArray(parts)) add(partsFile, '-', 'not an array', 'write an array of { id, title, line }');
    else parts.forEach((part, index) => {
      const field = `[${index}]`;
      if (!isObject(part)) return add(partsFile, field, 'not an object', 'write { id, title, line }');
      unknownKeys(partsFile, field, part, PART_KEYS);
      for (const key of PART_KEYS) if (!isText(part[key])) add(partsFile, at(field, key), 'missing or empty', `write the part's ${key}`);
      if (typeof part.id !== 'string') return;
      if (!ID_PATTERN.test(part.id)) add(partsFile, at(field, 'id'), `"${part.id}" is not an id`, 'use lowercase letters, digits and dashes');
      else if (RESERVED_PARTS.has(part.id)) add(partsFile, at(field, 'id'), `"${part.id}" is not allowed as a part id`, 'choose another id');
      if (partIds.has(part.id)) add(partsFile, at(field, 'id'), `"${part.id}" appears twice`, 'give each part its own id');
      partIds.add(part.id);
      if (!partFields.has(part.id)) partFields.set(part.id, at(field, 'id'));
    });
  }

  // The folder: the known files only, entry files named by an id, notes beside their entry.
  for (const name of LISTS) if (!files.has(name)) add(`plan/${name}`, '-', 'missing', `create plan/${name}`);
  for (const name of ['README.md', 'PRODUCT.md']) if (!files.has(name)) add(`plan/${name}`, '-', 'missing', `write plan/${name}`);
  for (const id of partIds) if (!files.has(`${id}/README.md`)) add(`plan/${id}/README.md`, '-', 'missing: every part has its page', `write plan/${id}/README.md`);
  /** @type {{ id: string, kind: 'decision' | 'feature', part: string | null, name: string, file: string }[]} */
  const entryFiles = [];
  for (const name of names) {
    if (ROOT_FILES.has(name)) continue;
    const partFile = PART_FILE.exec(name);
    if (partFile && partIds.has(partFile[1])) continue;
    const entry = ENTRY_FILE.exec(name);
    if (!entry || (entry[1] === undefined ? entry[2] !== 'decisions' : !partIds.has(entry[1]))) {
      add(`plan/${name}`, '-', 'not a file of the plan', 'move it out of plan/; plan/README.md lists its files, and a part\'s folder is named by an id of parts.json');
      continue;
    }
    const [, part = null, folder, id, ext] = entry;
    if (!ID_PATTERN.test(id)) {
      add(`plan/${name}`, '-', `"${id}" is not an id (${ID_PATTERN.source})`, 'rename the file: lowercase letters, digits and dashes');
      continue;
    }
    if (ext === 'md') {
      const own = name.replace(/\.md$/, '.json');
      if (!files.has(own)) add(`plan/${name}`, '-', 'notes without an entry file', `create plan/${own} or delete the notes`);
      continue;
    }
    entryFiles.push({ id, kind: folder === 'decisions' ? 'decision' : 'feature', part, name, file: `plan/${name}` });
  }

  /**
   * The shape of a statement: its keys, its required keys and its values (rules 2 and 3).
   * @param {'word' | 'principle' | 'property'} kind @param {string[]} [extra] keys required besides the statement's (`in` for a `sets` item)
   */
  function checkStatement(kind, file, field, statement, extra = []) {
    if (!isObject(statement)) return add(file, field, 'not an object', `write a ${kind} as an object`);
    unknownKeys(file, field, statement, [...extra, ...STATEMENT_KEYS[kind]]);
    present(file, field, statement, [...extra, ...REQUIRED_KEYS[kind]]);
    for (const key of STATEMENT_KEYS[kind]) if (!['not', 'check', 'at'].includes(key)) textKey(file, field, statement, key);
    const id = statementId(kind, statement);
    if (typeof id === 'string' && id !== '' && !ID_PATTERN.test(id)) add(file, at(field, idField(kind)), `"${id}" is not an id`, 'use lowercase letters, digits and dashes (in a word: spaces too)');
    if (kind === 'word') textList(file, at(field, 'not'), statement.not, 'words or phrases');
    if (kind === 'property') {
      if (Object.hasOwn(statement, 'kind') && !PROPERTY_KINDS.includes(statement.kind)) add(file, at(field, 'kind'), `"${shown(statement.kind)}" is not a kind`, `write one of ${PROPERTY_KINDS.join(', ')}`);
      for (const check of textList(file, at(field, 'check'), statement.check, 'checks')) {
        if (!CHECKS.includes(check)) add(file, at(field, 'check'), `"${check}" is not a check`, `write one or more of ${CHECKS.join(', ')}`);
      }
      textList(file, at(field, 'at'), statement.at, 'paths');
    }
  }

  // The lists of statements.
  /** @type {{ id: string, kind: 'word' | 'principle' | 'property', list: string, file: string, field: string, value: any }[]} */
  const statements = [];
  /** @type {[string, 'word' | 'principle' | 'property', string][]} file, kind, list name */
  const lists = [['words.json', 'word', 'words'], ['principles.json', 'principle', 'principles']];
  for (const name of names) {
    const match = /^(?:([^/]+)\/)?properties\.json$/.exec(name);
    if (match && (match[1] === undefined || partIds.has(match[1]))) lists.push([name, 'property', match[1] ? `${match[1]}/properties` : 'properties']);
  }
  for (const [name, kind, list] of lists) {
    const value = json.get(name);
    if (value === undefined) continue;
    const file = `plan/${name}`;
    if (!Array.isArray(value)) {
      add(file, '-', 'not an array', `write an array of ${kind}s`);
      continue;
    }
    value.forEach((statement, index) => {
      const field = `[${index}]`;
      checkStatement(kind, file, field, statement);
      if (!isObject(statement)) return;
      const id = statementId(kind, statement);
      if (isText(id)) statements.push({ id, kind, list, file, field, value: statement });
    });
  }

  // The entries: their shape (rules 2 to 5).
  /** @type {Map<string, any>} */
  const entries = new Map();
  for (const { id, kind, part, name, file } of entryFiles) {
    const data = json.get(name);
    if (data === undefined) continue;
    if (!isObject(data)) {
      add(file, '-', 'not an object', `write the ${kind} as an object`);
      continue;
    }
    const keys = kind === 'decision' ? DECISION_KEYS : FEATURE_KEYS;
    unknownKeys(file, '', data, keys);
    present(file, '', data, REQUIRED_KEYS[kind]);
    for (const key of ['asks', 'answer', 'name', 'what']) if (keys.includes(key)) textKey(file, '', data, key);
    for (const key of ['needs', 'replaces', 'removes', 'adds', 'keeps']) if (keys.includes(key)) textList(file, key, data[key], 'ids');
    if (Object.hasOwn(data, 'verdict') && !VERDICTS.includes(data.verdict)) add(file, 'verdict', `"${shown(data.verdict)}" is not a verdict`, `write ${VERDICTS.join(' or ')}, or leave it out`);
    if (Object.hasOwn(data, 'suggest') && !isEmpty(data.suggest)) {
      if (!isObject(data.suggest)) add(file, 'suggest', 'not an object', 'write { verdict, why }');
      else {
        unknownKeys(file, 'suggest', data.suggest, SUGGEST_KEYS);
        present(file, 'suggest', data.suggest, SUGGEST_KEYS);
        if (Object.hasOwn(data.suggest, 'verdict') && !VERDICTS.includes(data.suggest.verdict)) add(file, 'suggest.verdict', `"${shown(data.suggest.verdict)}" is not a verdict`, `write ${VERDICTS.join(' or ')}`);
        textKey(file, 'suggest', data.suggest, 'why');
      }
    }
    if (Object.hasOwn(data, 'suggested_in') && !(Number.isInteger(data.suggested_in) && data.suggested_in > 0)) add(file, 'suggested_in', 'not an issue number', 'write a positive integer');
    if (Object.hasOwn(data, 'surfaces') && !isEmpty(data.surfaces)) {
      if (!isObject(data.surfaces)) add(file, 'surfaces', 'not an object', `write { ${SURFACES_KEYS.join(', ')} }`);
      else {
        unknownKeys(file, 'surfaces', data.surfaces, SURFACES_KEYS);
        present(file, 'surfaces', data.surfaces, []);
        for (const key of SURFACES_KEYS) textKey(file, 'surfaces', data.surfaces, key);
      }
    }
    if (Object.hasOwn(data, 'built') && !isDate(data.built)) add(file, 'built', `"${shown(data.built)}" is not a date`, 'write the day it was built, YYYY-MM-DD');
    // Rule 5: sets and removes only before take, adds only with it. `replaces` stays after a take.
    const taken = data.verdict === 'take';
    if (taken && Object.hasOwn(data, 'sets')) add(file, 'sets', 'sets on a taken entry', 'run plan take, which writes the statements into their lists and records their ids in adds');
    if (taken && Object.hasOwn(data, 'removes')) add(file, 'removes', 'removes on a taken entry', 'run plan take, which removes the statements');
    if (!taken && Object.hasOwn(data, 'adds')) add(file, 'adds', 'adds without the verdict take', 'remove adds: only a taken entry has brought statements');
    if (Object.hasOwn(data, 'sets') && !isEmpty(data.sets)) {
      if (!Array.isArray(data.sets)) add(file, 'sets', 'not a list', 'write a list of { in, …the statement }');
      else data.sets.forEach((item, index) => {
        const field = `sets[${index}]`;
        if (!isObject(item)) return add(file, field, 'not an object', 'write { in, …the statement }');
        const kind = listKind(item.in);
        if (!kind) return add(file, at(field, 'in'), `"${shown(item.in)}" is not a list`, 'write words, principles, properties or <part>/properties');
        if (item.in.includes('/') && !partIds.has(item.in.split('/')[0])) add(file, at(field, 'in'), `"${item.in.split('/')[0]}" is not a part`, 'name a part of parts.json');
        checkStatement(kind, file, field, item, ['in']);
      });
    }
    if (!entries.has(id)) entries.set(id, { ...data, id, kind, part, file });
  }

  // Every entry file counts as an entry for the links, even one that does not parse: that one error is enough.
  const entryIds = new Set(entryFiles.map((entry) => entry.id));

  // Rule 1: one namespace of ids, across entries and statements.
  /** @type {Map<string, string>} where each id is first defined */
  const owner = new Map();
  for (const entry of entryFiles) {
    if (owner.has(entry.id)) add(entry.file, '-', `the id "${entry.id}" is also ${owner.get(entry.id)}`, 'give each decision, feature, word, principle and property its own id');
    else owner.set(entry.id, entry.file);
  }
  for (const statement of statements) {
    if (owner.has(statement.id)) add(statement.file, at(statement.field, idField(statement.kind)), `the id "${statement.id}" is also ${owner.get(statement.id)}`, 'give each decision, feature, word, principle and property its own id');
    else owner.set(statement.id, `${statement.file} ${statement.field}`);
  }
  /** @type {Map<string, typeof statements[number]>} */
  const statementById = new Map();
  for (const statement of statements) if (!statementById.has(statement.id)) statementById.set(statement.id, statement);
  // plan show takes a part's id as it takes an entry's or a statement's, so a part may not share one.
  // A word may: a part is often named by the word for its concept, and plan show on the part shows the word.
  for (const [id, field] of partFields) {
    const entry = entryFiles.find((item) => item.id === id);
    const statement = statementById.get(id);
    if (entry) add(partsFile, field, `the part id "${id}" is also ${entry.file}`, 'give the part or the entry another id');
    else if (statement && statement.kind !== 'word') add(partsFile, field, `the part id "${id}" is also the ${statement.kind} at ${statement.file} ${statement.field}`, `give the part or the ${statement.kind} another id`);
  }
  // A statement an entry sets joins the namespace when taken: it may replace or move a statement of its own kind only.
  for (const entry of entries.values()) {
    if (!Array.isArray(entry.sets)) continue;
    entry.sets.forEach((item, index) => {
      const kind = isObject(item) ? listKind(item.in) : null;
      if (!kind) return;
      const id = statementId(kind, item);
      if (typeof id !== 'string' || !ID_PATTERN.test(id)) return;
      const field = at(`sets[${index}]`, idField(kind));
      if (entries.has(id)) add(entry.file, field, `the id "${id}" is also ${entries.get(id).file}`, 'give the statement its own id');
      else if (kind !== 'word' && partIds.has(id)) add(entry.file, field, `the id "${id}" is also a part of plan/parts.json`, `give the ${kind} its own id`);
      else if (statementById.has(id) && statementById.get(id).kind !== kind) add(entry.file, field, `the id "${id}" is already a ${statementById.get(id).kind} of ${statementById.get(id).list}`, `give the ${kind} its own id: a sets item replaces or moves a statement of its own kind only`);
    });
  }

  // Rule 6: every statement is added by exactly one taken entry; every added id is a statement.
  /** @type {Map<string, string[]>} */
  const addedBy = new Map();
  for (const entry of entries.values()) {
    if (entry.verdict !== 'take' || !Array.isArray(entry.adds)) continue;
    entry.adds.forEach((id, index) => {
      if (typeof id !== 'string') return;
      if (!statementById.has(id)) add(entry.file, `adds[${index}]`, `"${id}" is not a statement`, 'add only the ids of words, principles and properties in their lists');
      addedBy.set(id, [...(addedBy.get(id) ?? []), entry.id]);
    });
  }
  for (const statement of statements) {
    const by = addedBy.get(statement.id) ?? [];
    const field = at(statement.field, idField(statement.kind));
    if (by.length === 0) add(statement.file, field, `"${statement.id}" is added by no taken entry`, 'name it in the adds of the taken decision or feature that brought it');
    else if (by.length > 1) add(statement.file, field, `"${statement.id}" is added by ${by.join(', ')}`, 'keep it in the adds of one entry only');
  }

  // Rule 7: links name what exists.
  const principleIds = new Set(statements.filter((s) => s.kind === 'principle').map((s) => s.id));
  const propertyIds = new Set(statements.filter((s) => s.kind === 'property').map((s) => s.id));
  for (const statement of statements) {
    if (statement.kind !== 'property') continue;
    if (!Object.hasOwn(statement.value, 'from')) notes.push({ file: statement.file, field: statement.field, message: `the property "${statement.id}" follows from no principle`, fix: 'name the principle it follows from in from, once there is one' });
    else if (typeof statement.value.from === 'string' && !principleIds.has(statement.value.from)) add(statement.file, at(statement.field, 'from'), `"${statement.value.from}" is not a principle`, 'name a principle of principles.json');
  }
  for (const entry of entries.values()) {
    const named = (key) => (Array.isArray(entry[key]) ? entry[key].map((id, index) => [id, `${key}[${index}]`]).filter(([id]) => typeof id === 'string') : []);
    for (const [id, field] of named('needs')) {
      if (id === entry.id) add(entry.file, field, `"${id}" needs itself`, 'remove it');
      else if (!entryIds.has(id)) add(entry.file, field, `"${id}" is not a decision or a feature`, 'name an entry of the plan');
    }
    for (const [id, field] of named('keeps')) if (!propertyIds.has(id)) add(entry.file, field, `"${id}" is not a property`, 'name a property of the plan');
    for (const [id, field] of named('removes')) if (!statementById.has(id)) add(entry.file, field, `"${id}" is not a statement`, 'name a word, principle or property of the plan');
    // A property it sets may follow from a principle in force, or from one this same entry sets.
    const ownPrinciples = new Set((Array.isArray(entry.sets) ? entry.sets : []).filter((item) => isObject(item) && listKind(item.in) === 'principle' && typeof item.id === 'string').map((item) => item.id));
    if (Array.isArray(entry.sets)) entry.sets.forEach((item, index) => {
      if (isObject(item) && listKind(item.in) === 'property' && typeof item.from === 'string' && !principleIds.has(item.from) && !ownPrinciples.has(item.from)) {
        add(entry.file, `sets[${index}].from`, `"${item.from}" is not a principle`, 'name a principle of principles.json, or one this entry sets');
      }
    });
    // Rule 5: a taken decision has every decision it needs taken.
    if (entry.kind === 'decision' && entry.verdict === 'take') {
      for (const [id, field] of named('needs')) {
        const need = entries.get(id);
        if (need?.kind === 'decision' && need.verdict !== 'take') add(entry.file, field, `"${id}" is not taken, and this taken decision needs it`, `take "${id}" first, or remove the verdict`);
      }
    }
  }

  // `replaces`: a taken decision takes the place of taken decisions, each replaced by one decision only.
  const decisionIds = new Set(entryFiles.filter((entry) => entry.kind === 'decision').map((entry) => entry.id));
  /** @type {Map<string, string[]>} the taken decisions that name each taken decision in `replaces` */
  const replacers = new Map();
  /** @type {[any, string, string][]} entry, the id it names, its field: the names that are a decision other than itself */
  const replacing = [];
  for (const entry of entries.values()) {
    if (entry.kind !== 'decision' || !Array.isArray(entry.replaces)) continue;
    // An id named twice is reported once, by the shape of the list.
    const seen = new Set();
    entry.replaces.forEach((id, index) => {
      if (typeof id !== 'string' || seen.has(id)) return;
      seen.add(id);
      const field = `replaces[${index}]`;
      if (id === entry.id) return add(entry.file, field, `"${id}" replaces itself`, 'remove it');
      if (!decisionIds.has(id)) return add(entry.file, field, `"${id}" is not a decision`, 'name a taken decision of the plan');
      replacing.push([entry, id, field]);
      const target = entries.get(id);
      if (target && target.verdict !== 'take') add(entry.file, field, `"${id}" is not taken`, `edit "${id}" instead: it has no verdict to replace`);
      else if (target && entry.verdict === 'take') replacers.set(id, [...(replacers.get(id) ?? []), entry.id]);
    });
  }
  /** @type {Map<string, string>} each replaced decision, and the decision that replaced it */
  const replacedBy = new Map([...replacers].map(([id, by]) => [id, by[0]]));
  for (const [entry, id, field] of replacing) {
    const by = replacers.get(id) ?? [];
    if (entry.verdict === 'take' && by.length > 1) add(entry.file, field, `"${id}" is replaced by ${by.join(', ')}`, `a decision is replaced by one decision only: keep "${id}" in the replaces of one of them`);
    if (entry.verdict === undefined && by.length) notes.push({ file: entry.file, field, message: `"${id}" is already replaced by ${by[0]}`, fix: `name "${by[0]}" instead, the decision that replaced it` });
  }
  for (const [id, by] of replacedBy) {
    const target = entries.get(id);
    if (Array.isArray(target.adds) && target.adds.length) add(target.file, 'adds', `"${id}" is replaced by ${by} and still adds ${target.adds.map(shown).join(', ')}`, `${by} sets again or removes each of those statements`);
  }
  // A chain of replaces among taken decisions never comes back to its start.
  /** @type {Map<string, string[]>} the taken decisions each taken decision replaces */
  const replacedOf = new Map();
  for (const [target, by] of replacers) for (const id of by) replacedOf.set(id, [...(replacedOf.get(id) ?? []), target]);
  for (const entry of entries.values()) {
    if (!replacedOf.has(entry.id)) continue;
    /** @type {Map<string, string>} each decision reached, and the one it was reached from */
    const from = new Map();
    const queue = [entry.id];
    let loop = false;
    while (queue.length && !loop) {
      const current = /** @type {string} */ (queue.shift());
      for (const next of replacedOf.get(current) ?? []) {
        if (next === entry.id) {
          from.set(next, current);
          loop = true;
          break;
        }
        if (!from.has(next)) {
          from.set(next, current);
          queue.push(next);
        }
      }
    }
    if (!loop) continue;
    const chain = [entry.id];
    for (let at = /** @type {string} */ (from.get(entry.id)); at !== entry.id; at = /** @type {string} */ (from.get(at))) chain.unshift(at);
    chain.unshift(entry.id);
    add(entry.file, 'replaces', `the replaces come back to "${entry.id}": ${chain.join(' replaces ')}`, 'remove one of them, so that a decision never replaces itself through others');
  }
  // A taken decision has an answer or an adds, unless it is replaced: then it is no longer in force.
  for (const entry of entries.values()) {
    if (entry.kind === 'decision' && entry.verdict === 'take' && !replacedBy.has(entry.id) && !Object.hasOwn(entry, 'answer') && !Object.hasOwn(entry, 'adds')) {
      add(entry.file, 'answer', 'a taken decision without an answer or adds', 'write the answer the owner took, or remove the verdict');
    }
  }
  // An entry still to do that needs a replaced decision may depend on what is no longer in force.
  for (const entry of entries.values()) {
    if (entry.verdict === 'drop' || isDone(entry) || !Array.isArray(entry.needs)) continue;
    entry.needs.forEach((id, index) => {
      if (typeof id === 'string' && replacedBy.has(id)) notes.push({ file: entry.file, field: `needs[${index}]`, message: `"${id}" is replaced by ${replacedBy.get(id)}`, fix: `need "${replacedBy.get(id)}" instead, if this entry depends on what that one says` });
    });
  }

  // Rule 8: order.json.
  const orderFile = 'plan/order.json';
  const order = json.get('order.json');
  if (order !== undefined) {
    /** @type {Map<string, number>} position of each entry, from 1 across the flattened order */
    const position = new Map();
    if (!isObject(order)) add(orderFile, '-', 'not an object', 'write { stages: [...] }');
    else {
      unknownKeys(orderFile, '', order, ORDER_KEYS);
      if (!Array.isArray(order.stages)) add(orderFile, 'stages', 'not a list', 'write a list of { id, title, line, entries }');
      else {
        const stageIds = new Set();
        let count = 0;
        order.stages.forEach((stage, index) => {
          const field = `stages[${index}]`;
          if (!isObject(stage)) return add(orderFile, field, 'not an object', 'write { id, title, line, entries }');
          unknownKeys(orderFile, field, stage, STAGE_KEYS);
          for (const key of ['id', 'title', 'line']) if (!isText(stage[key])) add(orderFile, at(field, key), 'missing or empty', `write the stage's ${key}`);
          if (typeof stage.id === 'string') {
            if (!ID_PATTERN.test(stage.id)) add(orderFile, at(field, 'id'), `"${stage.id}" is not an id`, 'use lowercase letters, digits and dashes');
            if (stageIds.has(stage.id)) add(orderFile, at(field, 'id'), `"${stage.id}" appears twice`, 'give each stage its own id');
            stageIds.add(stage.id);
          }
          if (!Array.isArray(stage.entries)) return add(orderFile, at(field, 'entries'), 'not a list', 'write the ids of its entries, [] for none');
          stage.entries.forEach((id, i) => {
            count += 1;
            const entryField = `${field}.entries[${i}]`;
            if (typeof id !== 'string') return add(orderFile, entryField, 'not an id', 'write the id of a decision or a feature');
            if (!entryIds.has(id)) add(orderFile, entryField, `"${id}" is not a decision or a feature`, 'remove it or create its file');
            if (position.has(id)) add(orderFile, entryField, `"${id}" is placed twice`, 'keep it in one stage, once');
            else position.set(id, count);
          });
        });
      }
    }
    for (const entry of entries.values()) {
      if (!position.has(entry.id)) add(entry.file, '-', `"${entry.id}" is in no stage`, 'place it in a stage of order.json');
      if (!Array.isArray(entry.needs)) continue;
      entry.needs.forEach((id, index) => {
        const need = entries.get(id);
        if (!need || id === entry.id) return;
        if (position.has(id) && position.has(entry.id) && position.get(id) > position.get(entry.id)) add(orderFile, entry.id, `"${entry.id}" comes before "${id}", which it needs`, `move "${entry.id}" after "${id}" in order.json`);
        if (entry.verdict !== 'drop' && need.verdict === 'drop') add(entry.file, `needs[${index}]`, `"${id}" is dropped`, 'drop this entry too, or remove the need');
      });
    }
  }

  // Rule 9: built, and the files of every at.
  for (const entry of entries.values()) {
    if (!Object.hasOwn(entry, 'built')) continue;
    if (entry.verdict !== 'take') add(entry.file, 'built', 'built without the verdict take', 'take the feature first, or remove built');
    const waiting = (Array.isArray(entry.needs) ? entry.needs : []).filter((id) => !isDone(entries.get(id)));
    if (waiting.length) add(entry.file, 'built', `built while ${waiting.map(shown).join(', ')} ${waiting.length > 1 ? 'are' : 'is'} not done`, 'finish what it needs first, or remove built');
    if (Array.isArray(entry.adds)) for (const id of entry.adds) {
      const statement = statementById.get(id);
      if (statement?.kind === 'property' && !Object.hasOwn(statement.value, 'at')) add(statement.file, at(statement.field, 'at'), `the property "${id}" of the built feature "${entry.id}" has no at`, 'name the files of its checks in at');
    }
  }
  /** @type {[string, string, unknown][]} file, field, the at */
  const ats = statements.filter((s) => s.kind === 'property').map((s) => [s.file, at(s.field, 'at'), s.value.at]);
  for (const entry of entries.values()) {
    if (Array.isArray(entry.sets)) entry.sets.forEach((item, index) => { if (isObject(item) && listKind(item.in) === 'property') ats.push([entry.file, `sets[${index}].at`, item.at]); });
  }
  for (const [file, field, paths] of ats) {
    if (Array.isArray(paths)) paths.forEach((path, index) => {
      if (!isText(path)) return;
      const bad = badPath(path);
      if (bad) add(file, `${field}[${index}]`, `"${path}" ${bad}`, 'name a file of the repository by its path from the top, such as engine/test/rule.zig');
      else if (exists && !exists(path)) add(file, `${field}[${index}]`, `"${path}" is not a regular file of the repository`, 'name a file, not a directory nor a link that leads outside, by its path from the top');
    });
  }
  if (!exists) notes.push({ file: 'plan/', field: '-', message: 'the files named by at were not looked for', fix: 'run plan check in the repository' });

  // Rule 10: the words under not, in every JSON string value and every .md file, except inside a not list.
  /** @type {{ phrase: string, pattern: RegExp, word: string }[]} */
  const refused = [];
  for (const statement of statements) {
    if (statement.kind !== 'word' || !Array.isArray(statement.value.not)) continue;
    for (const phrase of statement.value.not) if (isText(phrase)) refused.push({ phrase, pattern: phrasePattern(phrase), word: statement.value.word });
  }
  if (refused.length) {
    const scan = (file, field, text) => {
      for (const { phrase, pattern, word } of refused) if (pattern.test(text)) add(file, field, `"${phrase}" is not a word of the plan`, `write "${word}" instead`);
    };
    const walk = (file, field, value) => {
      if (typeof value === 'string') scan(file, field || '-', value);
      else if (Array.isArray(value)) value.forEach((item, index) => walk(file, `${field}[${index}]`, item));
      else if (isObject(value)) for (const [key, item] of Object.entries(value)) if (key !== 'not') walk(file, at(field, key), item);
    };
    for (const name of names) {
      if (name.endsWith('.md')) scan(`plan/${name}`, '-', /** @type {string} */ (files.get(name)));
      else if (json.has(name)) walk(`plan/${name}`, '', json.get(name));
    }
  }

  return { errors, notes };
}

/** The keys whose change, with the verdict unchanged, is an error (rule 11). */
const ENTRY_TEXT_KEYS = ['asks', 'answer', 'name', 'what', 'replaces', 'sets', 'removes'];

/**
 * Whether two versions of an entry file differ in what the owner rules on: the verdict or a key of
 * rule 11. `adds` is left out: a take of another entry rewrites it when it moves a statement. A
 * version that is missing, not JSON or not an object counts as none, unlike any object.
 * @param {string | null} before @param {string | null} after the texts of the file
 */
export function ruledKeysDiffer(before, after) {
  const parse = (text) => {
    if (text === null) return null;
    try {
      const value = JSON.parse(text);
      return isObject(value) ? value : null;
    } catch {
      return null;
    }
  };
  const [a, b] = [parse(before), parse(after)];
  if (a === null || b === null) return a !== b;
  return ['verdict', ...ENTRY_TEXT_KEYS].some((key) => JSON.stringify(a[key]) !== JSON.stringify(b[key]));
}

/** @param {Map<string, string>} files @returns {Map<string, { file: string, value: any }>} the entries that parse, by id */
function entriesOf(files) {
  const out = new Map();
  for (const [name, text] of files) {
    if (isHidden(name)) continue;
    const match = ENTRY_FILE.exec(name);
    if (!match || match[4] !== 'json' || out.has(match[3])) continue;
    try {
      const value = JSON.parse(text);
      if (isObject(value)) out.set(match[3], { file: `plan/${name}`, value });
    } catch {
      // Not JSON: checkFiles reports it.
    }
  }
  return out;
}

/**
 * Rule 11: an entry with a verdict whose asks, answer, name, what, replaces, sets or removes changed while the
 * verdict stayed. An entry with no verdict may change freely; one that gains or loses a verdict too.
 * `ruledByOwner(id, file)` says whether the latest commit since the base that changed the entry's
 * verdict or one of those keys is the owner's verdict on it: the owner ruled again after the
 * change, and the rule does not apply.
 * @param {Map<string, string>} files the plan as it is @param {Map<string, string>} baseFiles the merged plan
 * @param {{ ruledByOwner?: (id: string, file: string) => boolean }} [options]
 * @returns {{ errors: Problem[] }}
 */
export function checkChange(files, baseFiles, { ruledByOwner } = {}) {
  /** @type {Problem[]} */
  const errors = [];
  const base = entriesOf(baseFiles);
  for (const [id, { file, value }] of entriesOf(files)) {
    const before = base.get(id)?.value;
    if (!before || before.verdict === undefined || before.verdict !== value.verdict) continue;
    const changed = ENTRY_TEXT_KEYS.filter((key) => JSON.stringify(before[key]) !== JSON.stringify(value[key]));
    if (changed.length && ruledByOwner?.(id, file)) continue;
    for (const key of changed) {
      errors.push({ file, field: key, message: `"${id}" changed its ${key} while its verdict stayed ${shown(before.verdict)}`, fix: 'remove the verdict, so the owner rules on the new text, or undo the change' });
    }
  }
  return { errors };
}

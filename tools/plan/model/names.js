// Names shared by every part of the plan: the id pattern, the kinds, the verdicts, the checks and
// the key orders of each kind, as the model in plan/README.md gives them.

/**
 * An id of an entry, a statement, a part or a stage. An entry's id is also its file name. Entries and
 * statements share one namespace: no decision, feature, word, principle or property shares an id.
 */
export const ID_PATTERN = /^[a-z0-9][a-z0-9-]{0,62}$/;

/** A day, `YYYY-MM-DD`. */
export const DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/;

/** What is to do: one file each, with a verdict and a place in the order. */
export const ENTRY_KINDS = Object.freeze(['decision', 'feature']);

/** What holds: one item of a list each, in force. */
export const STATEMENT_KINDS = Object.freeze(['word', 'principle', 'property']);

export const VERDICTS = Object.freeze(['take', 'drop']);

/** A property's `kind`: it never happens, it always ends up happening, or a number we commit to. */
export const PROPERTY_KINDS = Object.freeze(['safety', 'liveness', 'measured']);

/** A property's `check`: what proves it. A property lists one or more, each once. */
export const CHECKS = Object.freeze(['types', 'model', 'simulator', 'hostile test', 'live check', 'benchmark', 'test', 'assertion']);

/** Key orders of the canonical form. */
export const WORD_KEYS = Object.freeze(['word', 'means', 'not']);
export const PRINCIPLE_KEYS = Object.freeze(['id', 'says', 'asks', 'known_as', 'why']);
export const PROPERTY_KEYS = Object.freeze(['id', 'says', 'kind', 'from', 'check', 'at']);
export const DECISION_KEYS = Object.freeze(['asks', 'answer', 'needs', 'replaces', 'sets', 'removes', 'adds', 'suggest', 'suggested_in', 'verdict']);
export const FEATURE_KEYS = Object.freeze(['name', 'what', 'needs', 'sets', 'removes', 'adds', 'keeps', 'surfaces', 'suggest', 'suggested_in', 'verdict', 'built']);
export const SUGGEST_KEYS = Object.freeze(['verdict', 'why']);
export const SURFACES_KEYS = Object.freeze(['app', 'cli']);
export const PART_KEYS = Object.freeze(['id', 'title', 'line']);
export const ORDER_KEYS = Object.freeze(['stages']);
export const STAGE_KEYS = Object.freeze(['id', 'title', 'line', 'entries']);

/** The keys each kind must have. */
export const REQUIRED_KEYS = Object.freeze({
  word: Object.freeze(['word', 'means']),
  principle: Object.freeze(['id', 'says', 'asks']),
  property: Object.freeze(['id', 'says', 'kind', 'check']),
  decision: Object.freeze(['asks']),
  feature: Object.freeze(['name', 'what']),
});

/**
 * The statement kind of a list a `sets` item goes to (`words`, `principles`, `properties` or
 * `<part>/properties`), or null for anything else.
 * @param {unknown} list @returns {'word' | 'principle' | 'property' | null}
 */
export function listKind(list) {
  if (list === 'words') return 'word';
  if (list === 'principles') return 'principle';
  if (list === 'properties') return 'property';
  if (typeof list === 'string') {
    const [part, name, ...rest] = list.split('/');
    if (!rest.length && name === 'properties' && ID_PATTERN.test(part)) return 'property';
  }
  return null;
}

/** @param {string} word @returns {string} the word's id: lower case, a hyphen for each space. */
export function wordId(word) {
  return String(word).toLowerCase().replaceAll(' ', '-');
}

/** @param {unknown} text @returns {boolean} a real calendar day in `YYYY-MM-DD`. */
export function isDate(text) {
  if (typeof text !== 'string' || !DATE_PATTERN.test(text)) return false;
  const [y, m, d] = text.split('-').map(Number);
  // setUTCFullYear, not Date.UTC: Date.UTC reads a year below 100 as 1900 plus that year.
  const date = new Date(0);
  date.setUTCFullYear(y, m - 1, d);
  return date.getUTCFullYear() === y && date.getUTCMonth() === m - 1 && date.getUTCDate() === d;
}
